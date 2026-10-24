//! Explicit local preview. Raw ASR is never overwritten or automatically inserted.
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub working: bool,
    pub text: Option<String>,
    pub error: Option<String>,
    pub model: String,
    pub elapsed_ms: u64,
}
#[derive(Deserialize)]
struct Report {
    text: String,
    model: String,
    elapsed_ms: u64,
    backend: String,
    offloaded_layers: u32,
}
struct Inner {
    status: Status,
    epoch: u64,
}
#[derive(Clone)]
pub struct Corrector {
    state: Arc<Mutex<Inner>>,
    epoch: Arc<AtomicU64>,
    closing: Arc<AtomicBool>,
    worker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}
impl Corrector {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Inner {
                status: Status::default(),
                epoch: 0,
            })),
            epoch: Arc::new(AtomicU64::new(0)),
            closing: Arc::new(AtomicBool::new(false)),
            worker: Arc::new(Mutex::new(None)),
        }
    }
    pub fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut state) = self.state.lock() {
            let working = state.status.working;
            state.status = Status {
                working,
                ..Status::default()
            };
        }
    }
    /// Exit must wait until the pipe owner has killed and reaped its child.
    pub fn shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        self.cancel();
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            let _ = handle.join();
        }
    }
    fn status(&self) -> Status {
        self.state
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or_default()
    }
    fn start(
        &self,
        runtime: std::path::PathBuf,
        model: String,
        backend: String,
        raw: String,
        hint: Option<crate::context::Hint>,
    ) -> Result<Status, String> {
        if model.is_empty()
            || model.len() > 4096
            || !Path::new(&model).is_absolute()
            || !Path::new(&model).is_file()
        {
            return Err("補正モデルの絶対パスを指定して設定を保存してください。元の認識結果は保持しています。".into());
        }
        if !runtime.is_file() {
            return Err(
                "補正ランタイムがありません。利用ガイドのビルド手順を確認してください。".into(),
            );
        }
        // Serialize publication of the worker with shutdown. The worker itself
        // never takes this lock, so joining cannot prevent its cancellation.
        let mut handle = self
            .worker
            .lock()
            .map_err(|_| "補正状態を確認できません。")?;
        if self.closing.load(Ordering::Acquire) {
            return Err("アプリを終了しています。".into());
        }
        let epoch = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "補正状態を確認できません。")?;
            if state.status.working {
                return Err("補正処理の終了をお待ちください。".into());
            }
            let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
            state.epoch = epoch;
            state.status = Status {
                working: true,
                ..Status::default()
            };
            epoch
        };
        if let Some(previous) = handle.take() {
            let _ = previous.join();
        }
        let worker = self.clone();
        *handle = Some(std::thread::spawn(move || {
            let context = hint.and_then(|hint| hint.correction()).unwrap_or_default();
            let result = run(
                &runtime,
                &model,
                &backend,
                &raw,
                &context,
                &worker.epoch,
                epoch,
            );
            if let Ok(mut state) = worker.state.lock()
                && state.epoch == epoch
            {
                state.status.working = false;
                if worker.epoch.load(Ordering::Acquire) == epoch {
                    match result {
                        Ok(report) => {
                            state.status.text = Some(report.text);
                            state.status.model = report.model;
                            state.status.elapsed_ms = report.elapsed_ms;
                        }
                        Err(error) => state.status.error = Some(error),
                    }
                }
            }
        }));
        Ok(self.status())
    }
}
fn run(
    runtime: &Path,
    model: &str,
    backend: &str,
    raw: &str,
    context: &str,
    epoch: &AtomicU64,
    current: u64,
) -> Result<Report, String> {
    let failed = || {
        "ローカル補正に失敗しました。モデル・実行方式・入力上限を確認してください。元の認識結果は保持しています。".to_owned()
    };
    let input = serde_json::to_vec(&serde_json::json!({"transcript":raw,"context":context}))
        .map_err(|_| failed())?;
    if input.len() > 24_576 {
        return Err("補正入力が上限を超えました。元の認識結果は保持しています。".into());
    }
    if epoch.load(Ordering::Acquire) != current {
        return Err("補正を取り消しました。".into());
    }
    let mut child = Command::new(runtime)
        .arg(model)
        .arg(backend)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| failed())?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failed());
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failed());
    };
    // Pipes only: source text never enters argv, environment or temporary files.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(24_577).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        if epoch.load(Ordering::Acquire) != current || started.elapsed() > Duration::from_secs(75) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            let _ = reader.join();
            return Err("補正を中止しました。元の認識結果は保持しています。".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = reader.join();
                return Err(failed());
            }
        }
    };
    let written = writer.join().map_err(|_| failed())?.map_err(|_| failed());
    let output = reader.join().map_err(|_| failed())?.map_err(|_| failed())?;
    if !status.success() || written.is_err() || output.len() > 24_576 {
        return Err(failed());
    }
    let report: Report = serde_json::from_slice(&output).map_err(|_| failed())?;
    if report.text.len() > 8192
        || report.text.contains('\0')
        || report.backend != backend
        || (backend == "metal" && report.offloaded_layers == 0)
    {
        return Err(failed());
    }
    Ok(report)
}
#[tauri::command]
pub fn correction_status(corrector: tauri::State<'_, Corrector>) -> Status {
    corrector.status()
}
#[tauri::command]
pub fn correct_transcript(
    app: tauri::AppHandle,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    corrector: tauri::State<'_, Corrector>,
) -> Result<Status, String> {
    let preferences = crate::desktop::preferences(&app)?;
    // Validate configuration before consuming the one-shot context lease.
    if !Path::new(&preferences.correction_model_path).is_absolute()
        || !Path::new(&preferences.correction_model_path).is_file()
    {
        return Err("補正モデルの絶対パスを指定して設定を保存してください。".into());
    }
    let runtime = app
        .path()
        .resource_dir()
        .map_err(|_| "補正ランタイムの場所を確認できません。")?
        .join("bin/local-correction");
    let (raw, hint) = recorder.correction_input()?;
    corrector.start(
        runtime,
        preferences.correction_model_path,
        preferences.correction_backend,
        raw,
        hint,
    )
}
#[tauri::command]
pub fn copy_correction(corrector: tauri::State<'_, Corrector>) -> Result<(), String> {
    let state = corrector
        .state
        .lock()
        .map_err(|_| "補正状態を確認できません。")?;
    if state.status.working {
        return Err("補正処理の終了をお待ちください。".into());
    }
    let text = state
        .status
        .text
        .as_ref()
        .filter(|text| !text.is_empty())
        .ok_or("コピーする補正結果がありません。")?;
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.clone()))
        .map_err(|_| "補正結果をコピーできません。".into())
}
