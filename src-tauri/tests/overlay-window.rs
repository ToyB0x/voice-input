//! Native window regression, independent of WebView rendering and TCC grants.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/overlay_state.rs"]
mod overlay_state;
#[cfg(target_os = "macos")]
#[path = "../src/overlay_window.rs"]
mod overlay_window;
#[cfg(target_os = "macos")]
mod probe {
    use objc2::{
        class, msg_send,
        runtime::{AnyObject, Bool},
    };
    use std::{
        path::PathBuf,
        process::Command,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use tauri::Manager;
    fn on_main<T: Send + 'static>(
        app: &tauri::AppHandle,
        task: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        let handle = app.clone();
        app.run_on_main_thread(move || {
            let _ = tx.send(task(&handle));
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "native main-thread operation timed out".to_owned())?
    }
    fn front_pid() -> i32 {
        assert!(objc2::MainThreadMarker::new().is_some());
        unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
            if front.is_null() {
                0
            } else {
                msg_send![front, processIdentifier]
            }
        }
    }
    fn inspect(_app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
        let p = super::overlay_window::properties()?;
        if p.key
            || p.can_key
            || p.can_main
            || !p.ignores_mouse
            || !p.floating
            || !p.non_activating
            || p.visible != visible
        {
            return Err("invalid non-activating panel properties".into());
        }
        Ok(())
    }
    fn run(app: &tauri::AppHandle, fixture: PathBuf) -> Result<(), String> {
        on_main(app, |app| inspect(app, false))?;
        // The real app starts with its normal main window visible. Complete that
        // startup activation before testing background recording HUD ordering.
        let start = Instant::now();
        while !on_main(app, |app| {
            Ok(app
                .get_webview_window("main")
                .is_some_and(|window| window.is_focused().unwrap_or(false)))
        })? {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("normal main window did not finish startup activation".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        on_main(app, |app| {
            app.get_webview_window("main")
                .ok_or("main missing")?
                .hide()
                .map_err(|error| error.to_string())
        })?;
        let marker = std::env::temp_dir().join(format!("overlay-focus-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let launched = Command::new("/usr/bin/open")
            .arg("-n")
            .arg("-a")
            .arg(fixture)
            .arg("--args")
            .arg(&marker)
            .status()
            .map_err(|e| e.to_string())?;
        if !launched.success() {
            return Err("focus fixture launch failed".into());
        }
        let start = Instant::now();
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&marker)
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
            {
                break pid;
            }
            if start.elapsed() > Duration::from_secs(15) {
                return Err("focus fixture did not start".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let outcome = (|| {
            let start = Instant::now();
            loop {
                if on_main(app, move |_| Ok(front_pid()))? == pid {
                    break;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    return Err("focus fixture did not become frontmost".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // Separate startup/fixture focus settling and positioning from ordering.
            // Never reactivate the fixture after an overlay operation.
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(200));
                on_main(app, move |_| {
                    let actual = front_pid();
                    if actual != pid {
                        return Err(format!(
                            "foreground changed before overlay: expected={pid},actual={actual},probe={}",
                            std::process::id()
                        ));
                    }
                    Ok(())
                })?;
            }
            for (index, visible) in [true, false, true, false].into_iter().enumerate() {
                on_main(app, move |_| {
                    super::overlay_window::render(&crate::overlay_state::Status {
                        visible,
                        phase: if index == 0 {
                            "recording"
                        } else {
                            "correcting"
                        },
                        message: if index == 0 {
                            "録音中"
                        } else {
                            "ローカル補正中・入力は待機"
                        },
                        recording_ms: 1234,
                        stage_ms: 2450,
                        level: 0.02,
                    })
                })?;
                std::thread::sleep(Duration::from_millis(250));
                on_main(app, move |app| {
                    inspect(app, visible)?;
                    let before_capture = front_pid();
                    if before_capture != pid {
                        let (own_active, target_active) = unsafe {
                            let own: *mut AnyObject =
                                msg_send![class!(NSApplication), sharedApplication];
                            let target: *mut AnyObject = msg_send![class!(NSRunningApplication),runningApplicationWithProcessIdentifier:pid];
                            let own_active: Bool = msg_send![own, isActive];
                            let target_active: Bool = msg_send![target, isActive];
                            (own_active.as_bool(), target_active.as_bool())
                        };
                        return Err(format!(
                            "foreground changed before capture: visible={visible},expected={pid},actual={before_capture},probe={},ownActive={own_active},targetActive={target_active}",
                            std::process::id()
                        ));
                    }
                    if visible && let Some(root) = std::env::var_os("VOICE_INPUT_OVERLAY_EVIDENCE")
                    {
                        let path = std::path::PathBuf::from(root).join(if index == 0 {
                            "overlay-recording.png"
                        } else {
                            "overlay-correcting.png"
                        });
                        super::overlay_window::capture(&path)?;
                    }
                    if front_pid() != pid {
                        return Err(format!(
                            "ordering changed foreground at visible={visible}: expected={pid},actual={},probe={}",
                            front_pid(),
                            std::process::id()
                        ));
                    }
                    Ok(())
                })?;
            }
            on_main(app, |_| super::overlay_window::hide())?;
            Ok(())
        })();
        // Dedicated synthetic app only; no user application or content is touched.
        let _ = on_main(app, move |_| {
            unsafe {
                let target: *mut AnyObject = msg_send![class!(NSRunningApplication),runningApplicationWithProcessIdentifier:pid];
                if !target.is_null() {
                    let _: Bool = msg_send![target, terminate];
                }
            }
            Ok(())
        });
        let _ = std::fs::remove_file(marker);
        outcome
    }
    pub fn main() {
        let Some(fixture) = std::env::var_os("VOICE_INPUT_OVERLAY_FOCUS_TARGET") else {
            println!("SKIP: native overlay focus probe requires the explicit CI fixture");
            return;
        };
        let context = tauri::generate_context!();
        // Use the product window configuration, including normal startup activation.
        let app = tauri::Builder::default()
            .setup(|_| {
                super::overlay_window::initialize()?;
                Ok(())
            })
            .build(context)
            .expect("build native overlay probe");
        let mut fixture = Some(PathBuf::from(fixture));
        app.run(move|app,event| {
            if matches!(event,tauri::RunEvent::Ready) && let Some(fixture)=fixture.take() {
                let app=app.clone();
                std::thread::spawn(move|| {
                    let outcome=run(&app,fixture);
                    match &outcome {
                        Ok(())=>println!("{{\"status\":\"passed\",\"nonFocusable\":true,\"ignoresMouse\":true,\"frontmostPreserved\":true,\"showHideCycles\":2,\"nativeViewRendered\":true}}"),
                        Err(reason)=>eprintln!("native overlay probe failed: {reason}"),
                    }
                    app.exit(if outcome.is_ok(){0}else{1});
                });
            }
        });
    }
}
#[cfg(target_os = "macos")]
fn main() {
    probe::main();
}
#[cfg(not(target_os = "macos"))]
fn main() {}
