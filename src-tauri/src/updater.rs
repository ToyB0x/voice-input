//! Official Tauri updater. Only public release metadata and packages cross the network.
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Manager;
use tauri_plugin_updater::{Update, UpdaterExt};

pub const PUBLIC_KEY: &str = match option_env!("VOICE_INPUT_UPDATER_PUBLIC_KEY") {
    Some(key) => key,
    None => "",
};
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    configured: bool,
    working: bool,
    installing: bool,
    current_version: String,
    available_version: Option<String>,
    phase: String,
    message: String,
    downloaded: u64,
    total: Option<u64>,
}
struct Inner {
    status: Status,
    update: Option<Update>,
}
#[derive(Clone)]
pub struct Updates(Arc<Mutex<Inner>>);
impl Updates {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            status: Status {
                configured: !PUBLIC_KEY.trim().is_empty(),
                working: false,
                installing: false,
                current_version: env!("CARGO_PKG_VERSION").into(),
                available_version: None,
                phase: "idle".into(),
                message: if PUBLIC_KEY.trim().is_empty() {
                    "このビルドは更新署名キー未設定です。"
                } else {
                    "更新は未確認です。"
                }
                .into(),
                downloaded: 0,
                total: None,
            },
            update: None,
        })))
    }
    pub fn working(&self) -> bool {
        self.0.lock().map_or(true, |s| s.status.working)
    }
    pub fn installing(&self) -> bool {
        self.0.lock().map_or(true, |s| s.status.installing)
    }
    fn status(&self) -> Status {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
    fn end(&self, phase: &str, message: &str) {
        if let Ok(mut s) = self.0.lock() {
            s.status.working = false;
            s.status.installing = false;
            s.status.phase = phase.into();
            s.status.message = message.into();
        }
    }
    fn check(&self, app: tauri::AppHandle) -> Result<Status, String> {
        let activity = app.state::<crate::activity::Activity>();
        let _gate = activity.lock()?;
        if !PUBLIC_KEY.trim().is_empty() && !busy(&app)? {
            {
                let mut s = self.0.lock().map_err(|_| "更新状態を確認できません。")?;
                if s.status.working {
                    return Err("更新の確認中です。".into());
                }
                s.status.working = true;
                s.status.phase = "checking".into();
                s.status.message = "GitHub Releasesで更新を確認中…".into();
            }
            let owner = self.clone();
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let updater = handle
                        .updater_builder()
                        .timeout(Duration::from_secs(30))
                        .build()
                        .map_err(|_| ())?;
                    updater.check().await.map_err(|_| ())
                }
                .await;
                match result {
                    Ok(Some(update)) if allowed_url(&update.download_url) => {
                        if let Ok(mut state) = owner.0.lock() {
                            state.status.available_version = Some(update.version.clone());
                            state.update = Some(update);
                        }
                        owner.end(
                            "available",
                            "更新があります。音声と比較結果を破棄後、インストールできます。",
                        );
                    }
                    Ok(None) => {
                        if let Ok(mut s) = owner.0.lock() {
                            s.update = None;
                            s.status.available_version = None;
                        }
                        owner.end("current", "最新の公開版です。");
                    }
                    _ => owner.end(
                        "error",
                        "更新を確認できません。未公開・通信・署名設定を確認してください。",
                    ),
                }
            });
        } else if PUBLIC_KEY.trim().is_empty() {
            return Err("更新署名キーが未設定です。".into());
        } else {
            return Err("録音・比較などの処理終了をお待ちください。".into());
        }
        Ok(self.status())
    }
    fn install(&self, app: tauri::AppHandle) -> Result<Status, String> {
        let activity = app.state::<crate::activity::Activity>();
        let _gate = activity.lock()?;
        if busy(&app)? {
            return Err("すべての処理が終了するまでお待ちください。".into());
        }
        let recording = app.state::<crate::recorder::Recorder>().status()?;
        if recording.sample_count > 0
            || recording.recognition.transcript.is_some()
            || app.state::<crate::comparison::Comparisons>().has_results()
            || app
                .state::<crate::correction::Corrector>()
                .status()
                .text
                .is_some()
        {
            return Err(
                "再起動でデータを失わないよう、録音・比較結果を先に破棄してください。".into(),
            );
        }
        let update = {
            let mut s = self.0.lock().map_err(|_| "更新状態を確認できません。")?;
            if s.status.working {
                return Err("更新処理の終了をお待ちください。".into());
            }
            let update = s.update.clone().ok_or("先に更新を確認してください。")?;
            if !allowed_url(&update.download_url) {
                return Err("更新URLが不正です。".into());
            }
            s.status.working = true;
            s.status.installing = true;
            s.status.downloaded = 0;
            s.status.total = None;
            s.status.phase = "downloading".into();
            s.status.message = "署名付き更新を取得・検証し、インストール後に再起動します。".into();
            update
        };
        let owner = self.clone();
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let progress = owner.clone();
            let result = update
                .download(
                    move |bytes, total| {
                        if let Ok(mut s) = progress.0.lock() {
                            s.status.downloaded += bytes as u64;
                            s.status.total = total;
                        }
                    },
                    || {},
                )
                .await;
            match result {
                Ok(bytes) => {
                    if let Ok(mut s) = owner.0.lock() {
                        s.status.phase = "installing".into();
                    }
                    // Official installer verifies package signature and signed version again.
                    let result =
                        tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await;
                    if matches!(result, Ok(Ok(()))) {
                        handle.restart();
                    } else {
                        owner.end("error","更新を適用できません。Applicationsへ移動し、権限・署名設定を確認してください。");
                    }
                }
                Err(_) => owner.end(
                    "error",
                    "更新の取得または署名検証に失敗しました。再試行してください。",
                ),
            }
        });
        Ok(self.status())
    }
}
fn allowed_url(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url
            .path()
            .starts_with("/ToyB0x/voice-input/releases/download/")
}
fn busy(app: &tauri::AppHandle) -> Result<bool, String> {
    let recording = app.state::<crate::recorder::Recorder>().status()?;
    Ok(matches!(recording.phase, "recording" | "preparing")
        || recording.recognition.working
        || app.state::<crate::comparison::Comparisons>().working()
        || app.state::<crate::correction::Corrector>().status().working
        || ["starting", "downloading", "verifying"]
            .contains(&app.state::<local_models::Manager>().status().phase.as_str()))
}
pub fn start_automatic_checks(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Do not compete with startup model loading. A busy check is retried on the next interval.
        let mut next = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            // Tauri's runtime runs this blocking timer away from UI and inference owners.
            let _ = tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(Duration::from_secs(30))
            })
            .await;
            if std::time::Instant::now() >= next {
                let updates = handle.state::<Updates>();
                if updates.check(handle.clone()).is_ok() {
                    next = std::time::Instant::now() + Duration::from_secs(6 * 60 * 60);
                }
            }
        }
    });
}
#[tauri::command]
pub fn update_status(updates: tauri::State<'_, Updates>) -> Status {
    updates.status()
}
#[tauri::command]
pub fn check_updates(
    app: tauri::AppHandle,
    updates: tauri::State<'_, Updates>,
) -> Result<Status, String> {
    updates.check(app.clone())
}
#[tauri::command]
pub fn install_update(
    app: tauri::AppHandle,
    updates: tauri::State<'_, Updates>,
) -> Result<Status, String> {
    updates.install(app.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_urls_are_scoped_to_this_repository() {
        assert!(allowed_url(
            &"https://github.com/ToyB0x/voice-input/releases/download/v1.0.0/app.tar.gz"
                .parse()
                .unwrap()
        ));
        for url in [
            "http://github.com/ToyB0x/voice-input/releases/download/v1/app",
            "https://github.com/other/repo/releases/download/v1/app",
            "https://evil.example/ToyB0x/voice-input/releases/download/v1/app",
            "https://user@github.com/ToyB0x/voice-input/releases/download/v1/app",
        ] {
            assert!(!allowed_url(&url.parse().unwrap()));
        }
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn official_updater_signature() {
        let external =
            std::env::var_os("VOICE_INPUT_VERIFY_UPDATE_DIR").map(std::path::PathBuf::from);
        let (manifest, public, bytes) = if let Some(root) = external {
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("latest.json")).unwrap()).unwrap();
            let url = manifest["platforms"]["darwin-aarch64"]["url"]
                .as_str()
                .unwrap();
            let file = url.rsplit('/').next().unwrap();
            (
                manifest.clone(),
                std::env::var("VOICE_INPUT_UPDATER_PUBLIC_KEY").unwrap(),
                std::fs::read(root.join(file)).unwrap(),
            )
        } else {
            (
                serde_json::json!({"version":"99.0.0","url":"https://github.com/ToyB0x/voice-input/releases/download/v99.0.0/fixture.app.tar.gz","signature":include_str!("../tests/updater-fixture/payload.sig").trim()}),
                include_str!("../tests/updater-fixture/public.key")
                    .trim()
                    .to_owned(),
                include_bytes!("../tests/updater-fixture/payload.txt").to_vec(),
            )
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/latest.json", listener.local_addr().unwrap());
        let body = serde_json::to_vec(&manifest).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.package_info_mut().version = "0.0.0-0".parse().unwrap();
        context.config_mut().plugins.0.insert("updater".into(),serde_json::json!({"pubkey":public,"requireSignedVersion":true,"dangerousInsecureTransportProtocol":true,"endpoints":[endpoint]}));
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let updater = app
            .updater_builder()
            .target("darwin-aarch64")
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let update = tauri::async_runtime::block_on(updater.check())
            .unwrap()
            .unwrap();
        server.join().unwrap();
        update
            .verify(&bytes)
            .expect("Official plugin must accept the signed archive and its version");
        let mut damaged = bytes.clone();
        damaged[0] ^= 1;
        assert!(update.verify(&damaged).is_err());
        let mut wrong_version = update.clone();
        wrong_version.version = "9999.0.0".into();
        assert!(
            wrong_version.verify(&bytes).is_err(),
            "Manifest version tampering must fail"
        );
        let mut wrong_signature = update.clone();
        wrong_signature.signature = "invalid-signature".into();
        assert!(wrong_signature.verify(&bytes).is_err());
    }
}
