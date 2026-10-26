//! Local correction: explicit preview or an opt-in automatic completion. Raw ASR is retained.
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
    pub checking_foundation: bool,
    pub timings: Option<Timings>,
    pub foundation: Option<FoundationStatus>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub total_ms: u64,
    pub model_reused: bool,
    pub backend_ms: Option<u64>,
    pub model_load_ms: Option<u64>,
    pub context_ms: Option<u64>,
    pub prefill_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub validation_ms: Option<u64>,
}
#[derive(Deserialize)]
struct Performance {
    backend_ms: u64,
    model_load_ms: u64,
    context_ms: u64,
    prefill_ms: u64,
    generation_ms: u64,
    validation_ms: u64,
}
#[derive(Deserialize)]
struct Report {
    text: String,
    model: String,
    elapsed_ms: u64,
    backend: String,
    offloaded_layers: u32,
    #[serde(default)]
    performance: Option<Performance>,
    #[serde(default)]
    model_reused: bool,
    #[serde(skip)]
    total_ms: u64,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct FoundationStatus {
    protocol_version: u32,
    framework_imported: bool,
    os_supported: bool,
    pub(crate) availability: String,
    os_major: u32,
    os_minor: u32,
    os_patch: u32,
}
#[derive(Clone)]
pub(crate) struct AutomaticConfig {
    pub runtime: std::path::PathBuf,
    pub model: String,
    pub backend: String,
    pub reuse_model: bool,
    pub instruction: String,
}
impl AutomaticConfig {
    pub fn from_preferences(
        app: &tauri::AppHandle,
        preferences: &crate::settings::Preferences,
    ) -> Result<Option<Self>, String> {
        if !preferences.auto_correction {
            return Ok(None);
        }
        Ok(Some(Self {
            runtime: app
                .path()
                .resource_dir()
                .map_err(|_| "補正ヘルパーを確認できません。")?
                .join("bin/local-correction"),
            model: preferences.correction_model_path.clone(),
            reuse_model: preferences.reuse_correction_model,
            instruction: preferences.correction_instruction.clone(),
            backend: if preferences.correction_engine == "apple-foundation" {
                "foundation".into()
            } else {
                preferences.correction_backend.clone()
            },
        }))
    }
}
enum Outcome {
    Correction(Report),
    Foundation(FoundationStatus),
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
    pool: Arc<crate::correction_pool::Pool>,
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
            pool: Arc::new(crate::correction_pool::Pool::default()),
            worker: Arc::new(Mutex::new(None)),
        }
    }
    pub fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut state) = self.state.lock() {
            let working = state.status.working;
            state.status = Status {
                working,
                foundation: state.status.foundation.clone(),
                ..Status::default()
            };
        }
    }
    /// Exit must wait until the pipe owner has killed and reaped its child.
    pub fn shutdown(&self) {
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.closing.store(true, Ordering::Release);
        self.cancel();
        if let Some(handle) = worker.take() {
            let _ = handle.join();
        }
        self.pool.release();
    }
    pub fn cancel_and_release_model(&self) {
        self.cancel();
        self.pool.release();
    }
    pub fn release_model(&self) {
        self.pool.release();
    }
    pub fn expire_model(&self) {
        self.pool.tick();
    }
    pub(crate) fn generation(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub(crate) fn status(&self) -> Status {
        self.state
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or_default()
    }
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        runtime: std::path::PathBuf,
        model: String,
        backend: String,
        raw: String,
        hint: Option<crate::context::Hint>,
        generation: u64,
        reuse_model: bool,
        instruction: String,
    ) -> Result<Status, String> {
        if backend != "foundation"
            && (model.is_empty()
                || model.len() > 4096
                || !Path::new(&model).is_absolute()
                || !Path::new(&model).is_file())
        {
            return Err("補正モデルの絶対パスを指定して設定を保存してください。元の認識結果は保持しています。".into());
        }
        if !runtime.is_file() {
            return Err(
                "補正ランタイムがありません。利用ガイドのビルド手順を確認してください。".into(),
            );
        }
        self.start_job(generation, false, move |worker, epoch| {
            let context = hint.and_then(|hint| hint.correction()).unwrap_or_default();
            let result = run_with_pool(
                reuse_model.then_some(worker.pool.as_ref()),
                &runtime,
                &model,
                &backend,
                &raw,
                &context,
                &instruction,
                &worker.epoch,
                epoch,
            );
            if result.is_err() {
                worker.pool.release();
            }
            result.map(Outcome::Correction)
        })
    }
    pub(crate) fn start_automatic(
        &self,
        config: AutomaticConfig,
        raw: String,
        hint: Option<crate::context::Hint>,
        generation: u64,
        completion: impl FnOnce(Result<String, String>) + Send + 'static,
    ) -> Result<Status, String> {
        // Reserve the existing cancellable worker before any fallible model operation.
        // A cancelled/busy reservation never falls back to insertion.
        self.start_job(generation, false, move |owner, epoch| {
            let context = hint.and_then(|hint| hint.correction()).unwrap_or_default();
            let result = run_with_pool(
                config.reuse_model.then_some(owner.pool.as_ref()),
                &config.runtime,
                &config.model,
                &config.backend,
                &raw,
                &context,
                &config.instruction,
                &owner.epoch,
                epoch,
            );
            if result.is_err() {
                owner.pool.release();
            }
            if owner.epoch.load(Ordering::Acquire) == epoch
                && !owner.closing.load(Ordering::Acquire)
            {
                completion(
                    result
                        .as_ref()
                        .map(|report| report.text.clone())
                        .map_err(Clone::clone),
                );
            }
            result.map(Outcome::Correction)
        })
    }
    fn start_job(
        &self,
        generation: u64,
        checking: bool,
        task: impl FnOnce(&Self, u64) -> Result<Outcome, String> + Send + 'static,
    ) -> Result<Status, String> {
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
            let epoch = generation.wrapping_add(1);
            self.epoch
                .compare_exchange(generation, epoch, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| "録音または設定が変わったため補正を取り消しました。")?;
            state.epoch = epoch;
            let mut status = if checking {
                state.status.clone()
            } else {
                Status {
                    foundation: state.status.foundation.clone(),
                    ..Status::default()
                }
            };
            status.working = true;
            status.checking_foundation = checking;
            status.error = None;
            state.status = status;
            epoch
        };
        if let Some(previous) = handle.take() {
            let _ = previous.join();
        }
        let worker = self.clone();
        *handle = Some(std::thread::spawn(move || {
            let result = task(&worker, epoch);
            if let Ok(mut state) = worker.state.lock()
                && state.epoch == epoch
            {
                state.status.working = false;
                state.status.checking_foundation = false;
                if worker.epoch.load(Ordering::Acquire) == epoch {
                    match result {
                        Ok(Outcome::Foundation(status)) => state.status.foundation = Some(status),
                        Ok(Outcome::Correction(report)) => {
                            state.status.text = Some(report.text);
                            state.status.model = report.model;
                            state.status.elapsed_ms = report.total_ms;
                            let mut timings = Timings {
                                total_ms: report.total_ms,
                                model_reused: report.model_reused,
                                ..Timings::default()
                            };
                            if let Some(p) = report.performance {
                                timings.backend_ms = Some(p.backend_ms);
                                timings.model_load_ms = Some(p.model_load_ms);
                                timings.context_ms = Some(p.context_ms);
                                timings.prefill_ms = Some(p.prefill_ms);
                                timings.generation_ms = Some(p.generation_ms);
                                timings.validation_ms = Some(p.validation_ms);
                            }
                            state.status.timings = Some(timings);
                        }
                        Err(error) => state.status.error = Some(error),
                    }
                }
            }
        }));
        Ok(self.status())
    }
}
#[cfg(test)]
fn run(
    runtime: &Path,
    model: &str,
    backend: &str,
    raw: &str,
    context: &str,
    epoch: &AtomicU64,
    current: u64,
) -> Result<Report, String> {
    run_with_pool(
        None, runtime, model, backend, raw, context, "", epoch, current,
    )
}
#[allow(clippy::too_many_arguments)]
fn run_with_pool(
    pool: Option<&crate::correction_pool::Pool>,
    runtime: &Path,
    model: &str,
    backend: &str,
    raw: &str,
    context: &str,
    instruction: &str,
    epoch: &AtomicU64,
    current: u64,
) -> Result<Report, String> {
    let failed = || {
        "ローカル補正に失敗しました。モデル・実行方式・入力上限を確認してください。元の認識結果は保持しています。".to_owned()
    };
    let mut payload = serde_json::json!({"transcript":raw,"context":context});
    if !instruction.is_empty() {
        payload["instruction"] = instruction.into();
    }
    let input = serde_json::to_vec(&payload).map_err(|_| failed())?;
    if input.len() > 24_576 {
        return Err("補正入力が上限を超えました。元の認識結果は保持しています。".into());
    }
    if epoch.load(Ordering::Acquire) != current {
        return Err("補正を取り消しました。".into());
    }
    let started = Instant::now();
    let mut command = Command::new(if backend == "foundation" {
        runtime.with_file_name("apple-foundation-correction")
    } else {
        runtime.to_path_buf()
    });
    if backend == "foundation" {
        command.arg("--correct");
    } else {
        command.arg(model).arg(backend);
    }
    let result = if let Some(pool) = pool.filter(|_| backend != "foundation") {
        use std::os::unix::process::ExitStatusExt;
        let stdout = pool.request(
            runtime,
            model,
            backend,
            input,
            epoch,
            current,
            Duration::from_secs(150),
        )?;
        let success = serde_json::from_slice::<serde_json::Value>(&stdout)
            .is_ok_and(|v| v.get("text").is_some());
        std::process::Output {
            status: std::process::ExitStatus::from_raw(if success { 0 } else { 256 }),
            stdout,
            stderr: Vec::new(),
        }
    } else {
        child_output(
            &mut command,
            input,
            epoch,
            current,
            Duration::from_secs(150),
        )?
    };
    let status = result.status;
    let output = result.stdout;
    if !status.success() {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(9) {
            return Err("ローカル補正に失敗しました（OSがプロセスを停止）。空きメモリ・モデルサイズ・実行方式を確認してください。元の認識結果は保持しています。".into());
        }
        let failure: serde_json::Value = serde_json::from_slice(&output).unwrap_or_default();
        if matches!(
            failure["reason"].as_str(),
            Some("metal-device-unavailable" | "metal-simd-unavailable")
        ) {
            return Err("このMacのGPUは補正に必要な演算機能を利用できません。CPUを選んで設定を保存してください。元の認識結果は保持しています。".into());
        }
        if failure["error"] == "rejected" {
            return Err("補正結果が原文保持のチェックを満たさず棄却されました。元の認識結果を確認してください。".into());
        }
        if backend == "foundation" {
            return Err(foundation_failure(&failure));
        }
        let stage = match failure["stage"].as_str() {
            Some("input") => "入力の確認",
            Some("backend") => "実行方式の初期化",
            Some("model") => "モデルの読み込み",
            Some("prompt") => "補正入力の準備",
            Some("prefill") => "補正入力の処理",
            Some("generation") => "文章の生成",
            _ => "実行結果の確認",
        };
        return Err(format!(
            "ローカル補正に失敗しました（{stage}）。モデル・実行方式・入力上限を確認してください。元の認識結果は保持しています。"
        ));
    }
    let mut report: Report = serde_json::from_slice(&output).map_err(|_| failed())?;
    if report.elapsed_ms > 150_000
        || report.text.trim().is_empty()
        || report.text.len() > 8192
        || report.text.contains('\0')
        || report.backend != backend
        || (backend == "metal" && report.offloaded_layers == 0)
        || (["cpu", "foundation"].contains(&backend) && report.offloaded_layers != 0)
    {
        return Err(failed());
    }
    if backend == "foundation" {
        let input =
            serde_json::to_vec(&serde_json::json!({"transcript":raw,"candidate":report.text}))
                .map_err(|_| failed())?;
        let guard = child_output(
            Command::new(runtime).arg("--validate"),
            input,
            epoch,
            current,
            Duration::from_secs(150).saturating_sub(started.elapsed()),
        )?;
        if !guard.status.success() {
            let failure: serde_json::Value =
                serde_json::from_slice(&guard.stdout).unwrap_or_default();
            if failure["error"] == "rejected" {
                return Err(format!(
                    "補正結果が原文保持のチェックを満たさず棄却されました [{}]。元の認識結果は保持しています。",
                    guard_code(failure["reason"].as_str())
                ));
            }
            return Err("原文保持チェックを実行できません [guard-runtime-failed]。元の認識結果は保持しています。".into());
        }
        #[derive(Deserialize)]
        struct Validated {
            text: String,
        }
        let validated: Validated = serde_json::from_slice(&guard.stdout).map_err(|_| failed())?;
        if validated.text != report.text {
            return Err(failed());
        }
    }
    if epoch.load(Ordering::Acquire) != current {
        return Err("補正を取り消しました。".into());
    }
    report.total_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}
pub(crate) fn child_output(
    command: &mut Command,
    input: Vec<u8>,
    epoch: &AtomicU64,
    current: u64,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let failed = || {
        "補正ヘルパーを実行できません。診断とビルド手順を確認してください。元の認識結果は保持しています。".to_owned()
    };
    if epoch.load(Ordering::Acquire) != current {
        return Err("補正を取り消しました。".into());
    }
    let mut child = command
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
        if epoch.load(Ordering::Acquire) != current || started.elapsed() > timeout {
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
    if written.is_err() || output.len() > 24_576 {
        return Err(failed());
    }
    Ok(std::process::Output {
        status,
        stdout: output,
        stderr: Vec::new(),
    })
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
    // Snapshot before taking preferences or the transcript. Every invalidation
    // must still be current when start atomically publishes its generation.
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    if app.state::<crate::comparison::Comparisons>().working()
        || app.state::<crate::updater::Updates>().installing()
    {
        return Err("比較・更新の終了をお待ちください。".into());
    }
    let generation = corrector.generation();
    let preferences = crate::desktop::preferences(&app)?;
    // Validate configuration before consuming the one-shot context lease.
    let foundation = preferences.correction_engine == "apple-foundation";
    if !foundation
        && (!Path::new(&preferences.correction_model_path).is_absolute()
            || !Path::new(&preferences.correction_model_path).is_file())
    {
        return Err("補正モデルの絶対パスを指定して設定を保存してください。".into());
    }
    let runtime = app
        .path()
        .resource_dir()
        .map_err(|_| "補正ランタイムの場所を確認できません。")?
        .join("bin/local-correction");
    if foundation
        && !runtime
            .with_file_name("apple-foundation-correction")
            .is_file()
    {
        return Err(
            "Appleモデルのヘルパーがありません [helper-missing]。ビルド手順を確認してください。"
                .into(),
        );
    }
    let (raw, hint) = recorder.correction_input()?;
    corrector.start(
        runtime,
        preferences.correction_model_path,
        if foundation {
            "foundation".into()
        } else {
            preferences.correction_backend
        },
        raw,
        hint,
        generation,
        preferences.reuse_correction_model,
        preferences.correction_instruction,
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

pub(crate) fn foundation_failure(failure: &serde_json::Value) -> String {
    let reason = failure["reason"].as_str().unwrap_or("");
    let (code, guidance) = match reason {
        "osTooOld" => ("osTooOld", "macOS 26以降が必要です。"),
        "frameworkMissingFromSDK" => (
            "frameworkMissingFromSDK",
            "Xcode 26以降でヘルパーを再ビルドしてください。",
        ),
        "deviceNotEligible" => (
            "deviceNotEligible",
            "Apple Intelligenceに対応したMacが必要です。",
        ),
        "appleIntelligenceNotEnabled" => (
            "appleIntelligenceNotEnabled",
            "システム設定でApple Intelligenceを有効にしてください。",
        ),
        "modelNotReady" | "assets-unavailable" => (
            "modelNotReady",
            "OSのモデル準備完了を待って、利用可否を再確認してください。",
        ),
        "unsupportedLanguageOrLocale" | "unsupported-language-or-locale" => (
            "unsupportedLanguageOrLocale",
            "日本語を利用できるOS・モデルの設定を確認してください。",
        ),
        "context-window-exceeded" => (
            "context-window-exceeded",
            "発話を短くするか、補正への文脈利用を無効にして試してください。",
        ),
        "guardrail-violation" => (
            "guardrail-violation",
            "Appleモデルの安全制約により生成できませんでした。",
        ),
        "rate-limited" => ("rate-limited", "しばらく待ってから再試行してください。"),
        "decoding-failure" => (
            "decoding-failure",
            "構造化した補正結果を生成できませんでした。",
        ),
        "unsupported-guide" => (
            "unsupported-guide",
            "このOSでは補正の出力形式を利用できません。",
        ),
        "concurrent-requests" => (
            "concurrent-requests",
            "他のモデル処理の完了を待ってください。",
        ),
        "invalid-input" | "input-limit" => ("input-limit", "補正入力の上限を確認してください。"),
        "invalid-output" => ("invalid-output", "補正結果が空または上限を超えました。"),
        _ => (
            "generation-failed",
            "利用可否を再確認し、診断情報を確認してください。",
        ),
    };
    format!("Appleモデルの補正を完了できません [{code}]。{guidance} 元の認識結果は保持しています。")
}
pub(crate) fn guard_code(reason: Option<&str>) -> &'static str {
    match reason {
        Some("correction changed numeric facts") => "guard-numbers",
        Some("correction removed a negation marker" | "correction changed a negation target") => {
            "guard-negation"
        }
        Some("correction removed a contrast target") => "guard-contrast",
        Some("correction removed a sentence connective") => "guard-connective",
        Some(
            "correction omitted the explicit final repair"
            | "correction retained the discarded repair",
        ) => "guard-self-repair",
        Some("correction replaced or removed too much of the source") => "guard-content-loss",
        Some("correction expanded the source excessively") => "guard-expansion",
        _ => "guard-output",
    }
}
pub(crate) fn probe_foundation(
    helper: &Path,
    epoch: &AtomicU64,
    current: u64,
) -> Result<FoundationStatus, String> {
    let result = child_output(
        Command::new(helper).arg("--status"),
        Vec::new(),
        epoch,
        current,
        Duration::from_secs(10),
    )?;
    let failed = || {
        "Appleモデルの利用可否を確認できません [invalid-status]。ビルド手順とヘルパーの配置を確認してください。".to_owned()
    };
    if !result.status.success() {
        return Err(failed());
    }
    let status: FoundationStatus = serde_json::from_slice(&result.stdout).map_err(|_| failed())?;
    if status.protocol_version != 1
        || ![
            "available",
            "osTooOld",
            "frameworkMissingFromSDK",
            "deviceNotEligible",
            "appleIntelligenceNotEnabled",
            "modelNotReady",
            "unsupportedLanguageOrLocale",
            "unknownUnavailableReason",
            "unknownAvailability",
        ]
        .contains(&status.availability.as_str())
    {
        return Err(failed());
    }
    Ok(status)
}
#[tauri::command]
pub fn check_foundation_model(
    app: tauri::AppHandle,
    corrector: tauri::State<'_, Corrector>,
) -> Result<Status, String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    if app.state::<crate::comparison::Comparisons>().working()
        || app.state::<crate::updater::Updates>().installing()
    {
        return Err("比較・更新の終了をお待ちください。".into());
    }
    let generation = corrector.generation();
    let helper = app
        .path()
        .resource_dir()
        .map_err(|_| "補正ヘルパーの配置を確認できません。")?
        .join("bin/apple-foundation-correction");
    // Uses the same tracked worker as correction: recording/cancel/exit kills and reaps the probe too.
    corrector.start_job(generation, true, move |worker, epoch| {
        probe_foundation(&helper, &worker.epoch, epoch).map(Outcome::Foundation)
    })
}
#[tauri::command]
pub fn copy_foundation_diagnostics(
    app: tauri::AppHandle,
    corrector: tauri::State<'_, Corrector>,
) -> Result<(), String> {
    let preferences = crate::desktop::preferences(&app)?;
    let status = corrector.status();
    // No paths, model metadata, audio, transcript, context, or generated body.
    let diagnostic = serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"), "diagnostic_version": 2,
        "selected_engine": preferences.correction_engine, "availability": status.foundation,
        "working": status.working, "checking_availability": status.checking_foundation,
        "last_elapsed_ms": status.elapsed_ms, "last_error": status.error,
        "timings": status.timings,
        "reuse_model_enabled": preferences.reuse_correction_model,
        "recognition_ms": app.state::<crate::recorder::Recorder>().status()?.recognition.recognition_ms,
    });
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(diagnostic.to_string()))
        .map_err(|_| "診断情報をコピーできません。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_preview_request_cannot_reverse_recording_invalidation() {
        let corrector = Corrector::new();
        let generation = corrector.generation();
        // Represents a request paused after reading an old transcript, before
        // worker publication, while a recording/clear/retry invalidates it.
        corrector.cancel();
        let executable = std::env::current_exe().unwrap();
        assert!(
            corrector
                .start(
                    executable.clone(),
                    executable.to_string_lossy().into_owned(),
                    "cpu".into(),
                    "old transcript".into(),
                    None,
                    generation,
                    false,
                    String::new(),
                )
                .is_err()
        );
        assert!(!corrector.status().working);
        assert!(corrector.status().text.is_none());
        assert!(corrector.worker.lock().unwrap().is_none());
    }
    fn fixture_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("voice-afm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn executable(path: &Path, source: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, source).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn automatic_reuse_reports_timings_and_shutdown_reaps_the_idle_helper() {
        let root = fixture_root("resident-shutdown");
        let runtime = root.join("local-correction");
        executable(
            &runtime,
            r#"#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(sys.argv[0] + '.pid').write_text(str(os.getpid()))
for count, line in enumerate(sys.stdin, 1):
    request = json.loads(line)
    print(json.dumps(dict(text=request['transcript']+'。', model='synthetic fixture', backend='cpu', offloaded_layers=0,
        elapsed_ms=5, model_reused=count > 1, performance=dict(backend_ms=0, model_load_ms=0, context_ms=1,
        prefill_ms=1, generation_ms=2, validation_ms=1))), flush=True)
"#,
        );
        let corrector = Corrector::new();
        for (raw, reused) in [("first synthetic", false), ("second synthetic", true)] {
            let config = AutomaticConfig {
                runtime: runtime.clone(),
                model: runtime.to_string_lossy().into_owned(),
                backend: "cpu".into(),
                reuse_model: true,
                instruction: String::new(),
            };
            let (tx, rx) = std::sync::mpsc::channel();
            corrector
                .start_automatic(
                    config,
                    raw.into(),
                    None,
                    corrector.generation(),
                    move |result| {
                        tx.send(result).unwrap();
                    },
                )
                .unwrap();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
                format!("{raw}。")
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while corrector.status().working && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let status = corrector.status();
            assert!(!status.working);
            assert_eq!(status.text.as_deref(), Some(format!("{raw}。").as_str()));
            let timings = status.timings.unwrap();
            assert_eq!(timings.model_reused, reused);
            assert_eq!(timings.generation_ms, Some(2));
        }
        let pid = std::fs::read_to_string(root.join("local-correction.pid")).unwrap();
        corrector.shutdown();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            corrector
                .start_job(corrector.generation(), false, |_, _| unreachable!())
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn releasing_a_cancelled_resident_never_delivers_a_raw_fallback() {
        let root = fixture_root("resident-cancel-delivery");
        let runtime = root.join("local-correction");
        executable(
            &runtime,
            r#"#!/usr/bin/env python3
import os, pathlib, sys, time
sys.stdin.readline()
pathlib.Path(sys.argv[0] + '.pid').write_text(str(os.getpid()))
time.sleep(30)
"#,
        );
        let corrector = Corrector::new();
        let config = AutomaticConfig {
            runtime: runtime.clone(),
            model: runtime.to_string_lossy().into_owned(),
            backend: "cpu".into(),
            reuse_model: true,
            instruction: String::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        corrector
            .start_automatic(
                config,
                "synthetic input".into(),
                None,
                corrector.generation(),
                move |result| {
                    let _ = tx.send(result);
                },
            )
            .unwrap();
        let marker = root.join("local-correction.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists());
        // Recorder holds its capture/delivery lock while invalidating first.
        // Stopping the helper first could otherwise turn cancellation into fallback.
        corrector.cancel_and_release_model();
        while corrector.status().working && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!corrector.status().working);
        assert!(rx.try_recv().is_err());
        assert!(corrector.status().text.is_none());
        corrector.shutdown();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn foundation_candidates_use_the_bundled_rust_guard() {
        let root = fixture_root("guard");
        let runtime = root.join("local-correction");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/local-correction"),
            &runtime,
        )
        .unwrap();
        let helper = root.join("apple-foundation-correction");
        let epoch = AtomicU64::new(1);
        for (candidate, accepted) in [("会議は10時です。", true), ("会議は20時です。", false)]
        {
            let report = serde_json::json!({"text":candidate,"model":"Apple Foundation Models","backend":"foundation","offloaded_layers":0,"elapsed_ms":5});
            executable(
                &helper,
                &format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n", report),
            );
            let result = run(&runtime, "", "foundation", "会議は10時です", "", &epoch, 1);
            if accepted {
                assert_eq!(result.ok().unwrap().text, candidate);
            } else {
                assert!(result.err().unwrap().contains("guard-numbers"));
            }
        }
        // Native stderr and unknown error content must never enter UI/diagnostics.
        executable(
            &helper,
            "#!/bin/sh\ncat >/dev/null\necho PRIVATE-CANARY >&2\nprintf '%s' '{\"error\":\"runtime\",\"reason\":\"PRIVATE-CANARY\"}'\nexit 1\n",
        );
        let error = run(&runtime, "", "foundation", "会議は10時です", "", &epoch, 1)
            .err()
            .unwrap();
        assert!(error.contains("generation-failed") && !error.contains("PRIVATE-CANARY"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shutdown_cancels_and_reaps_an_availability_probe() {
        let root = fixture_root("probe-cancel");
        let helper = root.join("probe");
        let pid_file = root.join("child.pid");
        executable(
            &helper,
            &format!(
                "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 30\n",
                pid_file.display()
            ),
        );
        let corrector = Corrector::new();
        corrector
            .start_job(corrector.generation(), true, move |worker, epoch| {
                probe_foundation(&helper, &worker.epoch, epoch).map(Outcome::Foundation)
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let started = Instant::now();
        corrector.shutdown();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        assert!(!corrector.status().working);
        assert!(
            corrector
                .start_job(corrector.generation(), true, |_, _| unreachable!())
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn foundation_failures_use_fixed_diagnostic_codes() {
        for code in [
            "osTooOld",
            "frameworkMissingFromSDK",
            "deviceNotEligible",
            "appleIntelligenceNotEnabled",
            "modelNotReady",
            "context-window-exceeded",
            "guardrail-violation",
        ] {
            assert!(foundation_failure(&serde_json::json!({"reason":code})).contains(code));
        }
        assert!(
            !foundation_failure(&serde_json::json!({"reason":"PRIVATE-CANARY"}))
                .contains("PRIVATE")
        );
    }
}
