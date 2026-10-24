//! Native entry points share the recorder; the WebView need not be visible.
use crate::{
    recorder::{self, Recorder},
    settings::{self, Preferences},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct Inner {
    preferences: Preferences,
    registered: Option<Shortcut>,
    error: Option<String>,
}
pub struct Desktop {
    path: PathBuf,
    inner: Mutex<Inner>,
    toggling: AtomicBool,
    shortcut_armed: AtomicBool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    preferences: Preferences,
    shortcut_registered: bool,
    error: Option<String>,
}

fn parse_shortcut(value: &str) -> Result<Option<Shortcut>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    let shortcut: Shortcut = value
        .parse()
        .map_err(|e| format!("ショートカットの形式が不正です: {e}"))?;
    if !shortcut
        .mods
        .intersects(Modifiers::SUPER | Modifiers::CONTROL | Modifiers::ALT)
    {
        return Err(
            "ショートカットにはCommand、Control、Optionのいずれかを含めてください。".into(),
        );
    }
    Ok(Some(shortcut))
}

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn shortcut_event(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    let Some(desktop) = app.try_state::<Desktop>() else {
        return;
    };
    let active = desktop
        .inner
        .lock()
        .is_ok_and(|i| i.registered.as_ref() == Some(shortcut));
    if !active {
        return;
    }
    if release_action(&desktop.shortcut_armed, state) {
        toggle(app);
    }
}

fn release_action(armed: &AtomicBool, state: ShortcutState) -> bool {
    match state {
        ShortcutState::Pressed => {
            armed.store(true, Ordering::Release);
            false
        }
        ShortcutState::Released => armed.swap(false, Ordering::AcqRel),
    }
}

fn toggle(app: &AppHandle) {
    if app.state::<Desktop>().toggling.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let snapshot = app.state::<Recorder>().status()?;
            if snapshot.recognition.working {
                return Err("処理が終わるまでお待ちください。".into());
            }
            // Stopping must remain possible even if microphone permission was
            // revoked during capture. Only starting requests permission.
            if snapshot.phase != "recording" {
                if recorder::permission_status() != "authorized" {
                    show(&app);
                }
                if recorder::request_microphone_permission().await? != "authorized" {
                    return Err("マイクの許可が必要です。設定画面を確認してください。".into());
                }
            }
            app.state::<Recorder>().toggle().await.map(|_| ())
        }
        .await;
        let desktop = app.state::<Desktop>();
        if let Ok(mut inner) = desktop.inner.lock() {
            inner.error = result.err();
        }
        desktop.toggling.store(false, Ordering::Release);
        if desktop
            .inner
            .lock()
            .is_ok_and(|inner| inner.error.is_some())
        {
            show(&app);
        }
    });
}

#[tauri::command]
pub fn desktop_status(desktop: tauri::State<'_, Desktop>) -> Result<Status, String> {
    let inner = desktop.inner.lock().map_err(|e| e.to_string())?;
    Ok(Status {
        preferences: inner.preferences.clone(),
        shortcut_registered: inner.registered.is_some(),
        error: inner.error.clone(),
    })
}

#[tauri::command]
pub fn save_preferences(
    app: AppHandle,
    desktop: tauri::State<'_, Desktop>,
    mut preferences: Preferences,
) -> Result<Status, String> {
    preferences.model_path = preferences.model_path.trim().into();
    preferences.shortcut = preferences.shortcut.trim().into();
    preferences.validate()?;
    let new = parse_shortcut(&preferences.shortcut)?;
    let mut inner = desktop.inner.lock().map_err(|e| e.to_string())?;
    let old = inner.registered;
    if new != old
        && let Some(shortcut) = new
    {
        app.global_shortcut().register(shortcut).map_err(|e| {
            format!("ショートカットを登録できません。別の組み合わせを指定してください: {e}")
        })?;
    }
    if let Err(error) = settings::save(&desktop.path, &preferences) {
        if new != old
            && let Some(shortcut) = new
        {
            let _ = app.global_shortcut().unregister(shortcut);
        }
        return Err(error);
    }
    if new != old
        && let Some(shortcut) = old
        && let Err(error) = app.global_shortcut().unregister(shortcut)
    {
        let _ = settings::save(&desktop.path, &inner.preferences);
        if let Some(shortcut) = new {
            let _ = app.global_shortcut().unregister(shortcut);
        }
        return Err(format!("以前のショートカットを解除できません: {error}"));
    }
    app.state::<crate::context::Reader>()
        .configure(preferences.context_config());
    if inner.preferences.correction_model_path != preferences.correction_model_path
        || inner.preferences.correction_backend != preferences.correction_backend
        || inner.preferences.context_config() != preferences.context_config()
    {
        app.state::<crate::correction::Corrector>().cancel();
    }
    inner.preferences = preferences;
    inner.registered = new;
    inner.error = None;
    desktop.shortcut_armed.store(false, Ordering::Release);
    app.state::<crate::insertion::Inserter>()
        .configure(inner.preferences.auto_insert);
    Ok(Status {
        preferences: inner.preferences.clone(),
        shortcut_registered: new.is_some(),
        error: None,
    })
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join("settings.json");
    let (preferences, mut error) = match settings::load(&path) {
        Ok(settings) => (settings, None),
        Err(error) => (Preferences::default(), Some(error)),
    };
    let registered = match parse_shortcut(&preferences.shortcut).and_then(|shortcut| {
        if let Some(shortcut) = shortcut {
            app.global_shortcut().register(shortcut).map_err(|e| {
                format!("ショートカットを登録できません。設定で変更してください: {e}")
            })?;
        }
        Ok(shortcut)
    }) {
        Ok(shortcut) => shortcut,
        Err(e) => {
            error = Some(e);
            None
        }
    };
    app.manage(Desktop {
        path,
        inner: Mutex::new(Inner {
            preferences: preferences.clone(),
            registered,
            error,
        }),
        toggling: AtomicBool::new(false),
        shortcut_armed: AtomicBool::new(false),
    });
    app.state::<crate::insertion::Inserter>()
        .configure(preferences.auto_insert);
    app.state::<crate::context::Reader>()
        .configure(preferences.context_config());
    let state = MenuItem::with_id(app, "voice-state", "待機", false, None::<&str>)?;
    let toggle_item =
        MenuItem::with_id(app, "voice-toggle", "録音を開始 / 停止", true, None::<&str>)?;
    let show_item = MenuItem::with_id(
        app,
        "voice-show",
        "設定・認識結果を表示",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "voice-quit", "Voice Inputを終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&state, &toggle_item, &show_item, &quit])?;
    TrayIconBuilder::with_id("voice-input")
        .title("VI 待機")
        .tooltip("Voice Input")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "voice-toggle" => toggle(app),
            "voice-show" => show(app),
            "voice-quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let mut previous = String::new();
        loop {
            if let Ok(snapshot) = handle.state::<Recorder>().status() {
                let label = if snapshot.phase == "recording" {
                    "録音中"
                } else if snapshot.recognition.working {
                    "処理中"
                } else if snapshot.phase == "error"
                    || snapshot.recognition.recognition_error.is_some()
                    || {
                        let status = handle.state::<crate::insertion::Inserter>().status();
                        status.enabled && status.state == "blocked"
                    }
                    || handle
                        .state::<Desktop>()
                        .inner
                        .lock()
                        .is_ok_and(|inner| inner.error.is_some())
                {
                    "要確認"
                } else {
                    "待機"
                };
                if previous != label {
                    previous = label.into();
                    let _ = state.set_text(label);
                    if let Some(tray) = handle.tray_by_id("voice-input") {
                        let _ = tray.set_title(Some(format!("VI {label}")));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    if !preferences.model_path.is_empty() {
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle
                .state::<Recorder>()
                .restore_model(preferences.model_path, preferences.backend)
                .await
                && let Ok(mut inner) = handle.state::<Desktop>().inner.lock()
            {
                inner.error = Some(error);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_actions_ignore_repeat_and_duplicate_events_and_recover_missing_releases() {
        use ShortcutState::{Pressed, Released};
        let armed = AtomicBool::new(false);
        let events = [
            Released, Pressed, Pressed, Released, Released, Pressed, Pressed, Released,
        ];
        let actions: Vec<_> = events
            .into_iter()
            .map(|event| release_action(&armed, event))
            .collect();
        assert_eq!(
            actions,
            [false, false, false, true, false, false, false, true]
        );
    }
    #[test]
    fn shortcuts_require_a_modifier_and_allow_disabling() {
        assert!(parse_shortcut("").unwrap().is_none());
        assert!(parse_shortcut("Command+Shift+Space").unwrap().is_some());
        assert!(parse_shortcut("Space").is_err());
        assert!(parse_shortcut("Shift+Space").is_err());
        assert!(parse_shortcut("Command+not-a-key").is_err());
    }
}

pub fn preferences(app: &AppHandle) -> Result<Preferences, String> {
    app.state::<Desktop>()
        .inner
        .lock()
        .map(|state| state.preferences.clone())
        .map_err(|_| "設定を確認できません。".into())
}
