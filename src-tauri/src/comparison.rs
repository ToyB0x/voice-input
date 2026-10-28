//! One recording, one frozen context, one child process at a time. No automatic insertion or files.
use local_comparison::{Case, Score, Selection, plan, score};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;
pub mod context_lab;
pub mod demo;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub index: usize,
    pub sample_id: Option<String>,
    pub case: Case,
    pub status: String,
    pub message: String,
    pub raw: Option<String>,
    pub candidate: Option<String>,
    pub final_text: Option<String>,
    pub accepted: Option<bool>,
    pub asr_ms: Option<u64>,
    pub asr_load_ms: Option<u64>,
    pub correction_ms: Option<u64>,
    pub total_ms: u64,
    pub asr_score: Option<Score>,
    pub candidate_score: Option<Score>,
    pub final_score: Option<Score>,
}
impl Row {
    fn new(index: usize, case: Case) -> Self {
        Self {
            index,
            sample_id: None,
            case,
            status: "unavailable".into(),
            message: String::new(),
            raw: None,
            candidate: None,
            final_text: None,
            accepted: None,
            asr_ms: None,
            asr_load_ms: None,
            correction_ms: None,
            total_ms: 0,
            asr_score: None,
            candidate_score: None,
            final_score: None,
        }
    }
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationDiagnostic {
    pub status: Option<crate::correction::FoundationStatus>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub generation: u64,
    pub mode: String,
    pub working: bool,
    pub armed: bool,
    pub phase: String,
    pub message: String,
    pub total: usize,
    pub completed: usize,
    pub audio_ms: u64,
    pub ax_characters: usize,
    pub ocr_characters: usize,
    pub has_reference: bool,
    pub foundation: Option<FoundationDiagnostic>,
    pub rows: Vec<Row>,
    pub next: usize,
    pub more: bool,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            generation: 0,
            mode: "recording".into(),
            working: false,
            armed: false,
            phase: "idle".into(),
            message: "録音済みの同じ音声を直列で比較します。".into(),
            total: 0,
            completed: 0,
            audio_ms: 0,
            ax_characters: 0,
            ocr_characters: 0,
            has_reference: false,
            foundation: None,
            rows: Vec::new(),
            next: 0,
            more: false,
        }
    }
}
struct Inner {
    status: Status,
    active: Option<u64>,
    lab: Option<context_lab::Session>,
    demo: demo::Session,
}
#[derive(Clone)]
pub struct Comparisons {
    inner: Arc<Mutex<Inner>>,
    epoch: Arc<AtomicU64>,
    armed: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    worker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}
impl Comparisons {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                status: Status::default(),
                active: None,
                lab: None,
                demo: demo::Session::default(),
            })),
            epoch: Arc::new(AtomicU64::new(0)),
            armed: Arc::new(AtomicBool::new(false)),
            closing: Arc::new(AtomicBool::new(false)),
            worker: Arc::new(Mutex::new(None)),
        }
    }
    pub fn working(&self) -> bool {
        self.inner.lock().map_or(true, |s| s.status.working)
    }
    pub fn has_results(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|s| !s.status.rows.is_empty() || s.lab.is_some() || !s.demo.rows.is_empty())
    }
    pub fn armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }
    pub fn cancel(&self, clear: bool) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut state) = self.inner.lock() {
            state.status.phase = if state.status.working {
                "cancelling"
            } else if clear {
                "idle"
            } else {
                "cancelled"
            }
            .into();
            if clear {
                state.lab = None;
                state.demo = demo::Session::default();
                state.status.rows.clear();
                state.status.foundation = None;
                state.status.audio_ms = 0;
                state.status.ax_characters = 0;
                state.status.ocr_characters = 0;
                state.status.has_reference = false;
                state.status.completed = 0;
                state.status.generation = self.epoch.load(Ordering::Acquire);
            }
        }
    }
    pub fn shutdown(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        self.closing.store(true, Ordering::Release);
        self.cancel(true);
        if let Some(handle) = worker.take() {
            let _ = handle.join();
        }
    }
    fn status(&self, generation: u64, after: usize) -> Status {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let original = &state.status;
        let mut status = Status {
            generation: original.generation,
            mode: original.mode.clone(),
            working: original.working,
            armed: self.armed(),
            phase: original.phase.clone(),
            message: original.message.clone(),
            total: original.total,
            completed: original.completed,
            audio_ms: original.audio_ms,
            ax_characters: original.ax_characters,
            ocr_characters: original.ocr_characters,
            has_reference: original.has_reference,
            foundation: original.foundation.clone(),
            rows: Vec::new(),
            next: 0,
            more: false,
        };
        let start = if generation == status.generation {
            after.min(original.rows.len())
        } else {
            0
        };
        let end = (start + 16).min(original.rows.len());
        status.next = end;
        status.more = end < original.rows.len();
        status.rows = original.rows[start..end].to_vec();
        status
    }
    // Called under the global startup gate, so no new recorder/corrector/model job can race reservation.
    fn start(
        &self,
        app: tauri::AppHandle,
        selection: Selection,
        recorder: crate::recorder::Recorder,
    ) -> Result<Status, String> {
        let cases = plan(&selection).map_err(str::to_owned)?;
        let mut handle = self
            .worker
            .lock()
            .map_err(|_| "比較ワーカーを確認できません。")?;
        if self.closing.load(Ordering::Acquire) || self.working() {
            return Err("比較処理の終了をお待ちください。".into());
        }
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| "比較状態を確認できません。")?;
            state.lab = None;
            state.active = Some(epoch);
            state.status = Status {
                generation: epoch,
                working: true,
                phase: "preparing".into(),
                total: cases.len(),
                has_reference: selection.reference.is_some(),
                ..Status::default()
            };
        }
        let input = match recorder.comparison_input() {
            Ok(input) => input,
            Err(error) => {
                self.finish(epoch, Some(error.clone()));
                return Err(error);
            }
        };
        if let Some(previous) = handle.take() {
            let _ = previous.join();
        }
        let owner = self.clone();
        *handle = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owner.run(&app, &recorder, &selection, &cases, input, epoch)
            }))
            .unwrap_or_else(|_| Err("比較ワーカーが終了しました。".into()));
            owner.finish(epoch, result.err());
        }));
        Ok(self.status(0, 0))
    }
    // Reserved under Activity, sharing cancellation and busy state with recorded comparisons.
    fn start_fixed(
        &self,
        app: tauri::AppHandle,
        recorder: crate::recorder::Recorder,
        include_qwen: bool,
    ) -> Result<Status, String> {
        recorder.ensure_comparison_idle()?;
        let preferences = crate::desktop::preferences(&app)?;
        let mut handle = self
            .worker
            .lock()
            .map_err(|_| "比較ワーカーを確認できません。")?;
        if self.closing.load(Ordering::Acquire) || self.working() {
            return Err("比較処理の終了をお待ちください。".into());
        }
        if let Some(previous) = handle.take() {
            let _ = previous.join();
        }
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| "比較状態を確認できません。")?;
            state.lab = None;
            state.active = Some(epoch);
            state.status = Status {
                generation: epoch,
                mode: "fixed-correction".into(),
                working: true,
                phase: "preparing".into(),
                total: FIXED_SAMPLES.len() * if include_qwen { 2 } else { 1 },
                ..Status::default()
            };
        }
        let owner = self.clone();
        *handle = Some(std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    // Release retained ASR weights without reading audio, transcript or context.
                    recorder.suspend_for_comparison()?;
                    if !owner.current(epoch) {
                        return Ok(());
                    }
                    let resources = app
                        .path()
                        .resource_dir()
                        .map_err(|_| "補正ヘルパーを確認できません。")?
                        .join("bin");
                    let paths = if include_qwen {
                        BTreeMap::from([(
                            "current-correction".into(),
                            custom_path(&preferences.correction_model_path),
                        )])
                    } else {
                        BTreeMap::new()
                    };
                    let apple = owner.foundation_check(&resources, epoch);
                    owner.message(
                        epoch,
                        "running",
                        "固定サンプルを直列で補正中。録音・画面情報は使用しません。",
                    );
                    run_fixed_samples(
                        &resources,
                        include_qwen,
                        &preferences.correction_backend,
                        &paths,
                        &apple,
                        &owner.epoch,
                        epoch,
                        |row| {
                            if let Ok(mut state) = owner.inner.lock()
                                && state.active == Some(epoch)
                                && owner.current(epoch)
                            {
                                state.status.completed = row.index + 1;
                                state.status.rows.push(row);
                            }
                        },
                    );
                    Ok(())
                }))
                .unwrap_or_else(|_| Err("自己診断ワーカーが終了しました。".into()));
            owner.finish(epoch, result.err());
        }));
        Ok(self.status(0, 0))
    }
    fn finish(&self, epoch: u64, error: Option<String>) {
        if let Ok(mut state) = self.inner.lock()
            && state.active == Some(epoch)
        {
            state.status.working = false;
            state.active = None;
            let stale = self.epoch.load(Ordering::Acquire) != epoch;
            let lab_complete =
                !stale && error.is_none() && state.status.completed == state.status.total;
            if let Some(lab) = &mut state.lab {
                lab.report["completed"] = lab_complete.into();
            }
            state.status.phase = if stale {
                "cancelled"
            } else if error.is_some() {
                "error"
            } else {
                "done"
            }
            .into();
            state.status.message = if stale {
                "比較を中止しました。".into()
            } else {
                error.unwrap_or_else(||"比較が完了しました。通常の音声入力を再開する場合は音声モデルを再読み込みしてください。".into())
            };
        }
    }
    fn current(&self, epoch: u64) -> bool {
        self.epoch.load(Ordering::Acquire) == epoch && !self.closing.load(Ordering::Acquire)
    }
    fn run(
        &self,
        app: &tauri::AppHandle,
        recorder: &crate::recorder::Recorder,
        selection: &Selection,
        cases: &[Case],
        input: crate::recorder::ComparisonInput,
        epoch: u64,
    ) -> Result<(), String> {
        // Drop the ordinary retained ASR model before loading any comparison model.
        recorder.suspend_for_comparison()?;
        if !self.current(epoch) {
            return Ok(());
        }
        let audio = local_transcription::prepare_audio(&input.audio, input.rate)
            .map_err(|_| "録音は1〜60秒の音声で比較してください。")?;
        let sources = input
            .hint
            .map(|h| h.comparison_sources())
            .unwrap_or_default();
        {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| "比較状態を確認できません。")?;
            state.status.audio_ms = audio.len() as u64 * 1000 / 16000;
            state.status.ax_characters = sources.ax.chars().count();
            state.status.ocr_characters = sources.ocr.chars().count();
        }
        let resources = app
            .path()
            .resource_dir()
            .map_err(|_| "比較ヘルパーを確認できません。")?
            .join("bin");
        let preferences = crate::desktop::preferences(app)?;
        let models = app.state::<local_models::Manager>();
        let mut paths: BTreeMap<String, Result<PathBuf, String>> = BTreeMap::new();
        for id in selection.speech.iter().chain(selection.corrections.iter()) {
            if !self.current(epoch) {
                return Ok(());
            }
            if ["none", "apple-foundation"].contains(&id.as_str()) || paths.contains_key(id) {
                continue;
            }
            self.message(
                epoch,
                "verifying-models",
                "比較に使うモデルを検証しています（測定時間には含めません）。",
            );
            let path = match id.as_str() {
                "current-speech" => custom_path(&preferences.model_path),
                "current-correction" => custom_path(&preferences.correction_model_path),
                _ => models
                    .comparison_path(id, || !self.current(epoch))
                    .map_err(str::to_owned),
            };
            paths.insert(id.clone(), path);
        }
        let apple = if selection
            .corrections
            .iter()
            .any(|id| id == "apple-foundation")
        {
            self.foundation_check(&resources, epoch)
        } else {
            Ok(())
        };
        for (index, case) in cases.iter().enumerate() {
            if !self.current(epoch) {
                break;
            }
            self.message(
                epoch,
                "running",
                &format!(
                    "{}/{} を直列実行中。録音・本文・文脈は送信しません。",
                    index + 1,
                    cases.len()
                ),
            );
            let row = run_case(
                &resources,
                case,
                index,
                &audio,
                &sources,
                &paths,
                &apple,
                selection.reference.as_deref(),
                &preferences.correction_instruction,
                &self.epoch,
                epoch,
            );
            if self.current(epoch)
                && let Ok(mut state) = self.inner.lock()
                && state.active == Some(epoch)
            {
                state.status.rows.push(row);
                state.status.completed = index + 1;
            }
        }
        Ok(())
    }
    fn foundation_check(&self, resources: &Path, epoch: u64) -> Result<(), String> {
        let mut apple = Ok(());
        {
            self.message(
                epoch,
                "checking-apple",
                "Appleモデルの利用可否を確認中（推論はまだ行いません）。",
            );
            let started = Instant::now();
            let result = crate::correction::probe_foundation(
                &resources.join("apple-foundation-correction"),
                &self.epoch,
                epoch,
            );
            let diagnostic = match result {
                Ok(status) => {
                    if status.availability != "available" {
                        apple = Err(format!("Apple FM: {}", status.availability));
                    }
                    FoundationDiagnostic {
                        status: Some(status),
                        error: None,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    }
                }
                Err(_) => {
                    apple = Err("foundation-probe-failed".into());
                    FoundationDiagnostic {
                        status: None,
                        error: Some("foundation-probe-failed".into()),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    }
                }
            };
            if let Ok(mut state) = self.inner.lock()
                && state.active == Some(epoch)
                && self.current(epoch)
            {
                state.status.foundation = Some(diagnostic);
            }
        }
        apple
    }
    fn message(&self, epoch: u64, phase: &str, message: &str) {
        if let Ok(mut state) = self.inner.lock()
            && state.active == Some(epoch)
            && self.current(epoch)
        {
            state.status.phase = phase.into();
            state.status.message = message.into();
        }
    }
}
fn custom_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || !path.is_file() {
        Err("モデルを設定・準備してください。".into())
    } else {
        Ok(path)
    }
}
// Public synthetic text only. Versioned IDs make reports comparable across app builds.
const FIXED_SAMPLES: [(&str, &str); 5] = [
    ("numbers", "明日の午後3時に会議室Bで10人の会議をします"),
    ("negation", "この変更は本番には適用しないでください"),
    ("self-repair", "金曜日、ではなく木曜日にお願いします"),
    (
        "filler-punctuation",
        "えーまず資料を確認して次に田中さんへ連絡します",
    ),
    ("list-format", "持ち物はパソコンと充電器と社員証です"),
];
#[allow(clippy::too_many_arguments)]
fn run_fixed_samples(
    resources: &Path,
    include_qwen: bool,
    backend: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
    mut publish: impl FnMut(Row),
) {
    for (sample_index, sample) in FIXED_SAMPLES.iter().enumerate() {
        for engine in if include_qwen {
            &["apple-foundation", "current-correction"][..]
        } else {
            &["apple-foundation"][..]
        } {
            if epoch.load(Ordering::Acquire) != current {
                return;
            }
            let index = sample_index * if include_qwen { 2 } else { 1 }
                + usize::from(*engine == "current-correction");
            let row = run_fixed_sample(
                resources, sample, index, engine, backend, paths, apple, epoch, current,
            );
            if epoch.load(Ordering::Acquire) != current {
                return;
            }
            publish(row);
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn run_fixed_sample(
    resources: &Path,
    sample: &(&str, &str),
    index: usize,
    engine: &str,
    backend: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
) -> Row {
    let case = Case {
        speech: "fixed-text".into(),
        speech_backend: "none".into(),
        correction: engine.into(),
        correction_backend: if engine == "apple-foundation" {
            "system".into()
        } else {
            backend.into()
        },
        source: "none".into(),
        route: "none".into(),
    };
    let mut row = Row::new(index, case.clone());
    row.sample_id = Some(sample.0.into());
    row.raw = Some(sample.1.into());
    row.final_text = row.raw.clone();
    let started = Instant::now();
    if let Err(error) = correct_text(
        resources, &case, &mut row, sample.1, "", "", paths, apple, epoch, current,
    ) {
        record_error(&mut row, error);
    }
    row.total_ms = started.elapsed().as_millis() as u64;
    row
}
#[derive(Deserialize)]
struct AsrReport {
    text: String,
    backend: String,
    load_ms: u64,
    elapsed_ms: u64,
}
#[derive(Deserialize)]
struct Candidate {
    candidate: String,
    accepted: bool,
    reason: Option<String>,
    backend: String,
    offloaded_layers: u32,
}
#[derive(Deserialize)]
struct AppleCandidate {
    text: String,
    backend: String,
    offloaded_layers: u32,
}
#[allow(clippy::too_many_arguments)] // Explicit immutable inputs shared by serial cases.
fn run_case(
    resources: &Path,
    case: &Case,
    index: usize,
    audio: &[f32],
    sources: &crate::context::Sources,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    reference: Option<&str>,
    instruction: &str,
    epoch: &AtomicU64,
    current: u64,
) -> Row {
    let mut row = Row::new(index, case.clone());
    let Some(context) = sources.for_source(&case.source) else {
        row.message="録音時の比較用文脈がありません。取得元の設定・権限と比較用文脈の保持を確認してください。".into();
        return row;
    };
    let Some(Ok(asr_path)) = paths.get(&case.speech) else {
        row.message = "音声モデルが未準備または検証に失敗しました。".into();
        return row;
    };
    let started = Instant::now();
    let outcome = (|| -> Result<(), String> {
        let hint = local_transcription::recognition_hint(if case.whisper_context() {
            context.as_str()
        } else {
            ""
        });
        let mut packet =
            serde_json::to_vec(&serde_json::json!({"samples":audio.len(),"hint":hint}))
                .map_err(|_| "input-failed")?;
        packet.push(b'\n');
        packet.reserve(audio.len() * 4);
        for sample in audio {
            packet.extend_from_slice(&sample.to_le_bytes());
        }
        let output = crate::correction::child_output(
            Command::new(resources.join("compare-transcription"))
                .arg(asr_path)
                .arg(&case.speech_backend),
            packet,
            epoch,
            current,
            Duration::from_secs(600),
        )?;
        row.asr_ms = Some(started.elapsed().as_millis() as u64);
        if !output.status.success() {
            let failure: serde_json::Value =
                serde_json::from_slice(&output.stdout).unwrap_or_default();
            return Err(match failure["error"].as_str() {
                Some("metal-unavailable" | "backend-unavailable") => "metal-unavailable",
                Some("model-load-failed") => "asr-model-load-failed",
                Some("invalid-pcm" | "input-limit") => "invalid-audio",
                _ => "asr-runtime-failed",
            }
            .into());
        }
        let asr: AsrReport =
            serde_json::from_slice(&output.stdout).map_err(|_| "invalid-asr-report")?;
        if asr.backend != case.speech_backend
            || asr.text.len() > 8192
            || asr.text.contains('\0')
            || asr.elapsed_ms < asr.load_ms
        {
            return Err("invalid-asr-report".into());
        }
        row.asr_load_ms = Some(asr.load_ms);
        row.raw = Some(asr.text.clone());
        row.final_text = Some(asr.text.clone());
        correct_text(
            resources,
            case,
            &mut row,
            &asr.text,
            &context,
            instruction,
            paths,
            apple,
            epoch,
            current,
        )
    })();
    row.total_ms = started.elapsed().as_millis() as u64;
    if let Err(error) = outcome {
        record_error(&mut row, error);
    }
    // The reference is used only here, never sent to either model.
    row.asr_score = row.raw.as_deref().and_then(|text| score(reference, text));
    row.candidate_score = row
        .candidate
        .as_deref()
        .and_then(|text| score(reference, text));
    row.final_score = row
        .final_text
        .as_deref()
        .and_then(|text| score(reference, text));
    row
}
#[allow(clippy::too_many_arguments)]
fn correct_text(
    resources: &Path,
    case: &Case,
    row: &mut Row,
    transcript: &str,
    context: &str,
    instruction: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
) -> Result<(), String> {
    correct_text_profile(
        resources,
        case,
        row,
        transcript,
        context,
        instruction,
        paths,
        apple,
        epoch,
        current,
        "grounded-structured-v2",
    )
}
#[allow(clippy::too_many_arguments)]
fn correct_text_profile(
    resources: &Path,
    case: &Case,
    row: &mut Row,
    transcript: &str,
    context: &str,
    instruction: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
    foundation_profile: &str,
) -> Result<(), String> {
    correct_text_input(
        resources,
        case,
        row,
        transcript,
        transcript,
        context,
        instruction,
        paths,
        apple,
        epoch,
        current,
        foundation_profile,
    )
}
#[allow(clippy::too_many_arguments)]
fn correct_text_input(
    resources: &Path,
    case: &Case,
    row: &mut Row,
    transcript: &str,
    correction_input: &str,
    context: &str,
    instruction: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
    foundation_profile: &str,
) -> Result<(), String> {
    let mut correction_started = None;
    let outcome = (|| -> Result<(), String> {
        if case.correction == "none" || transcript.trim().is_empty() {
            row.final_text = Some(transcript.to_owned());
            row.status = "uncorrected".into();
            row.accepted = None;
            return Ok(());
        }
        if case.correction == "apple-foundation"
            && let Err(reason) = apple
        {
            return Err(reason.clone());
        }
        if !["none", "apple-foundation"].contains(&case.correction.as_str())
            && !paths.get(&case.correction).is_some_and(Result::is_ok)
        {
            return Err("correction-model-missing".into());
        }
        correction_started = Some(Instant::now());
        let mut payload = serde_json::json!({"transcript":correction_input,"context":if case.correction_context(){context}else{""}});
        if !instruction.is_empty() {
            payload["instruction"] = instruction.into();
        }
        if case.correction == "apple-foundation" {
            payload["profile"] = foundation_profile.into();
        }
        let payload = serde_json::to_vec(&payload).map_err(|_| "input-failed")?;
        let candidate = if case.correction == "apple-foundation" {
            let output = crate::correction::child_output(
                Command::new(resources.join("apple-foundation-correction")).arg("--correct"),
                payload,
                epoch,
                current,
                Duration::from_secs(150),
            )?;
            if !output.status.success() {
                let error = serde_json::from_slice(&output.stdout).unwrap_or_default();
                return Err(crate::correction::foundation_failure(&error));
            }
            let apple: AppleCandidate =
                serde_json::from_slice(&output.stdout).map_err(|_| "invalid-correction-report")?;
            if apple.backend != "foundation"
                || apple.offloaded_layers != 0
                || apple.text.len() > 8192
                || apple.text.contains('\0')
            {
                return Err("invalid-correction-report".into());
            }
            let validation = serde_json::to_vec(
                &serde_json::json!({"transcript":transcript,"candidate":apple.text}),
            )
            .map_err(|_| "input-failed")?;
            let result = crate::correction::child_output(
                Command::new(resources.join("local-correction")).arg("--validate"),
                validation,
                epoch,
                current,
                Duration::from_secs(10),
            )?;
            if !result.status.success() {
                let failure: serde_json::Value =
                    serde_json::from_slice(&result.stdout).unwrap_or_default();
                if failure["error"] != "rejected" {
                    return Err("guard-runtime-failed".into());
                }
                Candidate {
                    candidate: apple.text,
                    accepted: false,
                    reason: Some(crate::correction::guard_code(failure["reason"].as_str()).into()),
                    backend: "foundation".into(),
                    offloaded_layers: 0,
                }
            } else {
                let checked: serde_json::Value =
                    serde_json::from_slice(&result.stdout).map_err(|_| "invalid-guard-report")?;
                if checked["text"].as_str() != Some(apple.text.as_str()) {
                    return Err("guard-output-mismatch".into());
                }
                Candidate {
                    candidate: apple.text,
                    accepted: true,
                    reason: None,
                    backend: "foundation".into(),
                    offloaded_layers: 0,
                }
            }
        } else {
            let path = paths
                .get(&case.correction)
                .and_then(|p| p.as_ref().ok())
                .ok_or("correction-model-missing")?;
            let output = crate::correction::child_output(
                Command::new(resources.join("local-correction"))
                    .arg(path)
                    .arg(&case.correction_backend)
                    .arg("--compare"),
                payload,
                epoch,
                current,
                Duration::from_secs(150),
            )?;
            if !output.status.success() {
                return Err("correction-runtime-failed".into());
            }
            let candidate: Candidate =
                serde_json::from_slice(&output.stdout).map_err(|_| "invalid-correction-report")?;
            if candidate.backend != case.correction_backend
                || (case.correction_backend == "metal" && candidate.offloaded_layers == 0)
                || (case.correction_backend == "cpu" && candidate.offloaded_layers != 0)
            {
                return Err("backend-mismatch".into());
            }
            candidate
        };
        if candidate.candidate.len() > 8192 || candidate.candidate.contains('\0') {
            return Err("invalid-correction-report".into());
        }
        row.accepted = Some(candidate.accepted);
        row.final_text = Some(if candidate.accepted {
            candidate.candidate.clone()
        } else {
            transcript.to_owned()
        });
        row.status = if candidate.accepted {
            "accepted"
        } else {
            "rejected"
        }
        .into();
        if !candidate.accepted {
            row.message = if case.correction == "apple-foundation" {
                candidate.reason.unwrap_or_else(|| "guard-output".into())
            } else {
                crate::correction::guard_code(candidate.reason.as_deref()).into()
            };
        }
        row.candidate = Some(candidate.candidate);
        Ok(())
    })();
    row.correction_ms = correction_started.map(|time| time.elapsed().as_millis() as u64);
    outcome
}
fn record_error(row: &mut Row, error: String) {
    row.status = if error == "metal-unavailable"
        || error.starts_with("Apple FM: ")
        || error == "correction-model-missing"
    {
        "unavailable"
    } else {
        "error"
    }
    .into();
    row.message = error;
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    id: String,
    name: String,
    kind: String,
    available: bool,
}
#[tauri::command]
pub fn comparison_models(app: tauri::AppHandle) -> Result<Vec<Choice>, String> {
    let prefs = crate::desktop::preferences(&app)?;
    let mut choices: Vec<_> = app
        .state::<local_models::Manager>()
        .models()
        .into_iter()
        .map(|m| Choice {
            id: m.id,
            name: m.name,
            kind: m.kind,
            available: m.cached,
        })
        .collect();
    choices.extend([
        Choice {
            id: "current-speech".into(),
            name: "設定中の音声モデル".into(),
            kind: "transcription".into(),
            available: Path::new(&prefs.model_path).is_file(),
        },
        Choice {
            id: "current-correction".into(),
            name: "設定中のQwenモデル".into(),
            kind: "correction".into(),
            available: Path::new(&prefs.correction_model_path).is_file(),
        },
        Choice {
            id: "none".into(),
            name: "補正なし".into(),
            kind: "correction".into(),
            available: true,
        },
        Choice {
            id: "apple-foundation".into(),
            name: "Apple Foundation Models（利用可否は比較時に確認）".into(),
            kind: "correction".into(),
            available: true,
        },
    ]);
    Ok(choices)
}
#[tauri::command]
pub fn comparison_plan(selection: Selection) -> Result<usize, String> {
    plan(&selection).map(|p| p.len()).map_err(str::to_owned)
}
#[tauri::command]
pub fn comparison_status(
    comparisons: tauri::State<'_, Comparisons>,
    generation: u64,
    after: usize,
) -> Status {
    comparisons.status(generation, after)
}
fn ensure_comparison_available(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<crate::updater::Updates>().working()
        || app.state::<crate::correction::Corrector>().status().working
        || ["starting", "downloading", "verifying"]
            .contains(&app.state::<local_models::Manager>().status().phase.as_str())
    {
        return Err("他の処理の終了をお待ちください。".into());
    }
    Ok(())
}
#[tauri::command]
pub fn start_comparison(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    selection: Selection,
) -> Result<Status, String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    ensure_comparison_available(&app)?;
    comparisons.start(app.clone(), selection, recorder.inner().clone())
}
#[tauri::command]
pub fn start_correction_selftest(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    include_qwen: bool,
) -> Result<Status, String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    ensure_comparison_available(&app)?;
    comparisons.start_fixed(app.clone(), recorder.inner().clone(), include_qwen)
}
#[tauri::command]
pub fn cancel_comparison(comparisons: tauri::State<'_, Comparisons>, clear: bool) {
    comparisons.cancel(clear);
}
#[tauri::command]
pub fn arm_comparison_context(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    enabled: bool,
) -> Result<(), String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    let snapshot = recorder.status()?;
    if enabled
        && (comparisons.working()
            || matches!(snapshot.phase, "recording" | "preparing")
            || snapshot.recognition.working)
    {
        return Err("処理を終えてから比較用文脈を有効にしてください。".into());
    }
    if !enabled {
        comparisons.cancel(true);
        recorder.clear_comparison_hint();
    }
    comparisons.armed.store(enabled, Ordering::Release);
    app.state::<crate::context::Reader>()
        .set_comparison(enabled);
    Ok(())
}
#[tauri::command]
pub fn copy_comparison_result(
    comparisons: tauri::State<'_, Comparisons>,
    generation: u64,
    index: usize,
) -> Result<(), String> {
    let state = comparisons
        .inner
        .lock()
        .map_err(|_| "比較状態を確認できません。")?;
    if state.status.working || state.status.generation != generation {
        return Err("比較結果を再確認してください。".into());
    }
    let text = state
        .status
        .rows
        .iter()
        .find(|r| r.index == index)
        .and_then(|r| r.final_text.as_ref())
        .filter(|s| !s.is_empty())
        .ok_or("コピーできる候補がありません。")?;
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.clone()))
        .map_err(|_| "候補をコピーできません。".into())
}

#[tauri::command]
pub fn copy_comparison_report(
    comparisons: tauri::State<'_, Comparisons>,
    generation: u64,
    include_text: bool,
) -> Result<(), String> {
    let report = {
        let state = comparisons
            .inner
            .lock()
            .map_err(|_| "比較状態を確認できません。")?;
        if state.status.working
            || state.status.generation != generation
            || state.status.rows.is_empty()
        {
            return Err("比較結果を再確認してください。".into());
        }
        comparison_report(&state.status, include_text)?
    };
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(report))
        .map_err(|_| "レポートをコピーできません。".into())
}

// Build from an allowlist: no audio, source context, reference or model paths are exported.
fn comparison_report(status: &Status, include_text: bool) -> Result<String, String> {
    let mut rows = serde_json::to_value(&status.rows).map_err(|_| "比較結果を読み出せません。")?;
    if !include_text && let Some(rows) = rows.as_array_mut() {
        for row in rows {
            if let Some(row) = row.as_object_mut() {
                for key in ["raw", "candidate", "finalText"] {
                    row.remove(key);
                }
            }
        }
    }
    serde_json::to_string_pretty(&serde_json::json!({
        "schemaVersion":1, "appVersion":env!("CARGO_PKG_VERSION"),
        "platform":std::env::consts::OS, "architecture":std::env::consts::ARCH,
        "mode":status.mode, "sampleSetVersion":if status.mode == "fixed-correction" {Some(1)} else {None},
        "phase":status.phase, "planned":status.total, "completed":status.completed,
        "audioMs":status.audio_ms, "hasReference":status.has_reference,
        "axCharacters":status.ax_characters, "ocrCharacters":status.ocr_characters,
        "foundation":status.foundation, "includesText":include_text, "rows":rows
    }))
    .map_err(|_| "レポートを作成できません。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn case() -> Case {
        Case {
            speech: "base".into(),
            speech_backend: "cpu".into(),
            correction: "none".into(),
            correction_backend: "none".into(),
            source: "none".into(),
            route: "none".into(),
        }
    }
    #[test]
    fn diagnostic_report_excludes_bodies_by_default_and_preserves_failure_context() {
        let mut row = Row::new(0, case());
        row.raw = Some("PRIVATE_TRANSCRIPT".into());
        row.candidate = Some("PRIVATE_CANDIDATE".into());
        row.final_text = row.raw.clone();
        row.status = "error".into();
        row.message = "generation-failed".into();
        let status = Status {
            total: 6,
            completed: 1,
            rows: vec![row],
            foundation: Some(FoundationDiagnostic {
                status: Some(
                    serde_json::from_value(serde_json::json!({
                        "protocol_version":1,"framework_imported":true,"os_supported":true,
                        "availability":"modelNotReady","os_major":27,"os_minor":0,"os_patch":1
                    }))
                    .unwrap(),
                ),
                error: None,
                elapsed_ms: 12,
            }),
            ..Status::default()
        };
        let report = comparison_report(&status, false).unwrap();
        assert!(!report.contains("PRIVATE_"));
        let report: serde_json::Value = serde_json::from_str(&report).unwrap();
        for key in ["raw", "candidate", "finalText"] {
            assert!(report["rows"][0].get(key).is_none());
        }
        assert_eq!(
            report["foundation"]["status"]["availability"],
            "modelNotReady"
        );
        assert_eq!(report["rows"][0]["message"], "generation-failed");
        assert_eq!(report["planned"], 6);
        assert!(
            comparison_report(&status, true)
                .unwrap()
                .contains("PRIVATE_TRANSCRIPT")
        );
    }
    #[test]
    fn pagination_resets_after_clear_and_cancel_waits_for_worker() {
        let owner = Comparisons::new();
        {
            let mut state = owner.inner.lock().unwrap();
            state.active = Some(1);
            state.status.generation = 1;
            state.status.working = true;
            state.status.rows = (0..20).map(|i| Row::new(i, case())).collect();
        }
        owner.epoch.store(1, Ordering::Release);
        let first = owner.status(0, 999);
        assert_eq!(first.rows.len(), 16);
        assert_eq!(first.next, 16);
        assert!(first.more);
        let second = owner.status(1, first.next);
        assert_eq!(second.rows.len(), 4);
        assert!(!second.more);
        owner.cancel(true);
        assert!(owner.working());
        assert!(!owner.has_results());
        assert!(owner.status(1, 20).rows.is_empty());
        owner.finish(1, None);
        assert!(!owner.working());
    }
    #[test]
    fn missing_context_is_not_silently_treated_as_no_context() {
        let sources = crate::context::Sources {
            ax: "AX fixture".into(),
            ocr: String::new(),
        };
        assert_eq!(sources.for_source("none"), Some(String::new()));
        assert_eq!(sources.for_source("ax"), Some("AX fixture".into()));
        assert!(sources.for_source("both").is_none());
        assert!(sources.for_source("ocr").is_none());
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(root: &Path, name: &str, script: &str) {
        let path = root.join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn fixed_samples_use_only_builtin_text_serially_and_do_not_invent_asr_scores() {
        let root = std::env::temp_dir().join(format!("voice-fixed-samples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("processes");
        let script = format!(
            r#"#!/usr/bin/python3
import sys,json,os
from pathlib import Path
log=Path({log:?})
if log.exists():
 try: os.kill(int(log.read_text()),0)
 except ProcessLookupError: pass
 else: raise AssertionError('previous process not reaped')
log.write_text(str(os.getpid()))
p=json.load(sys.stdin)
if sys.argv[1:] == ['--validate']:
 assert p['candidate']==p['transcript']+'。'
 print(json.dumps(dict(text=p['candidate'])))
else:
 assert p['context']==''
 assert p['transcript'] in {inputs}
 if sys.argv[1:] == ['--correct']:
  print(json.dumps(dict(text=p['transcript']+'。',backend='foundation',offloaded_layers=0)))
 else:
  assert sys.argv[-2:]==['cpu','--compare']
  print(json.dumps(dict(candidate=p['transcript']+'。',accepted=True,reason=None,backend='cpu',offloaded_layers=0)))
"#,
            log = log.to_str().unwrap(),
            inputs = serde_json::to_string(&FIXED_SAMPLES.iter().map(|s| s.1).collect::<Vec<_>>())
                .unwrap()
        );
        fixture(&root, "apple-foundation-correction", &script);
        fixture(&root, "local-correction", &script);
        let paths = BTreeMap::from([("current-correction".into(), Ok(root.join("model.gguf")))]);
        let mut rows = Vec::new();
        run_fixed_samples(
            &root,
            true,
            "cpu",
            &paths,
            &Ok(()),
            &AtomicU64::new(1),
            1,
            |row| rows.push(row),
        );
        assert_eq!(rows.len(), 10);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.index, index);
            assert_eq!(row.sample_id.as_deref(), Some(FIXED_SAMPLES[index / 2].0));
            assert_eq!(row.status, "accepted", "{}", row.message);
            assert_eq!(
                row.final_text.as_deref(),
                Some(format!("{}。", FIXED_SAMPLES[index / 2].1).as_str())
            );
            assert!(row.asr_ms.is_none() && row.asr_score.is_none() && row.final_score.is_none());
            assert!(row.correction_ms.is_some());
        }
        let status = Status {
            mode: "fixed-correction".into(),
            rows,
            total: 10,
            completed: 10,
            ..Status::default()
        };
        let report = comparison_report(&status, false).unwrap();
        assert!(!report.contains(FIXED_SAMPLES[0].1));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&report).unwrap()["sampleSetVersion"],
            1
        );
        let mut rows = Vec::new();
        run_fixed_samples(
            &root,
            false,
            "cpu",
            &paths,
            &Err("Apple FM: deviceNotEligible".into()),
            &AtomicU64::new(1),
            1,
            |row| rows.push(row),
        );
        assert_eq!(rows.len(), 5);
        assert!(
            rows.iter().all(|r| r.status == "unavailable"
                && r.raw == r.final_text
                && r.candidate.is_none())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancelling_fixed_samples_reaps_the_running_child_and_publishes_no_stale_rows() {
        let root = std::env::temp_dir().join(format!("voice-fixed-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("started");
        fixture(
            &root,
            "apple-foundation-correction",
            &format!(
                "#!/usr/bin/python3\nimport os,time,json,sys\njson.load(sys.stdin)\nopen({:?},'w').write(str(os.getpid()))\ntime.sleep(10)\n",
                marker.to_str().unwrap()
            ),
        );
        let epoch = Arc::new(AtomicU64::new(1));
        let child_epoch = epoch.clone();
        let child_root = root.clone();
        let worker = std::thread::spawn(move || {
            let mut rows = Vec::new();
            run_fixed_samples(
                &child_root,
                true,
                "cpu",
                &BTreeMap::new(),
                &Ok(()),
                &child_epoch,
                1,
                |row| rows.push(row),
            );
            rows
        });
        let started = Instant::now();
        while std::fs::read_to_string(&marker)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .is_none()
            && started.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists(), "helper did not start");
        epoch.fetch_add(1, Ordering::AcqRel);
        assert!(worker.join().unwrap().is_empty());
        let result = Command::new("/usr/bin/python3")
            .arg("-c")
            .arg("import os,sys; os.kill(int(sys.argv[1]),0)")
            .arg(std::fs::read_to_string(&marker).unwrap())
            .output()
            .unwrap();
        assert!(!result.status.success(), "cancelled child still running");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn apple_unavailability_failure_and_guard_rejection_keep_scored_asr() {
        let root =
            std::env::temp_dir().join(format!("voice-comparison-apple-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let paths = BTreeMap::from([("base".into(), Ok(root.join("speech.bin")))]);
        let sources = crate::context::Sources {
            ax: "CONTEXT_FIXTURE".into(),
            ocr: String::new(),
        };
        let case = Case {
            speech: "base".into(),
            speech_backend: "cpu".into(),
            correction: "apple-foundation".into(),
            correction_backend: "system".into(),
            source: "ax".into(),
            route: "correction-only".into(),
        };
        let asr = r#"#!/usr/bin/python3
import sys,json
header=json.loads(sys.stdin.buffer.readline())
assert header=={'samples':16000,'hint':''}
assert len(sys.stdin.buffer.read())==64000
print(json.dumps(dict(text='raw fixture',backend='cpu',load_ms=1,elapsed_ms=2)))
"#;
        let apple = r#"#!/usr/bin/python3
import sys,json
assert sys.argv[1:] == ['--correct']
p=json.load(sys.stdin)
assert p=={'transcript':'raw fixture','context':'CONTEXT_FIXTURE','profile':'grounded-structured-v2'}
print(json.dumps(dict(text='candidate fixture',backend='foundation',offloaded_layers=0)))
"#;
        let guard = r#"#!/usr/bin/python3
import sys,json
assert sys.argv[1:] == ['--validate']
assert json.load(sys.stdin)=={'transcript':'raw fixture','candidate':'candidate fixture'}
print(json.dumps(dict(error='rejected',reason='correction changed numeric facts')))
sys.exit(1)
"#;
        for (name, script) in [
            ("compare-transcription", asr),
            ("apple-foundation-correction", apple),
            ("local-correction", guard),
        ] {
            let path = root.join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        for mode in [
            "unavailable",
            "rejected",
            "generation-failed",
            "missing-helper",
        ] {
            if mode == "generation-failed" {
                std::fs::write(root.join("apple-foundation-correction"), "#!/usr/bin/python3\nimport sys,json\np=json.load(sys.stdin)\nprint(json.dumps(dict(error='PRIVATE_UNTRUSTED_ERROR')))\nsys.exit(1)\n").unwrap();
            }
            if mode == "missing-helper" {
                std::fs::remove_file(root.join("apple-foundation-correction")).unwrap();
            }
            let available = if mode == "unavailable" {
                Err("Apple FM: deviceNotEligible".into())
            } else {
                Ok(())
            };
            let row = run_case(
                &root,
                &case,
                0,
                &vec![0.0; 16000],
                &sources,
                &paths,
                &available,
                Some("raw fixture"),
                "",
                &AtomicU64::new(1),
                1,
            );
            assert_eq!(row.raw.as_deref(), Some("raw fixture"), "{mode}");
            assert_eq!(row.final_text, row.raw, "{mode}");
            assert_eq!(row.final_score.unwrap().cer, Some(0.0));
            assert!(!row.message.contains("PRIVATE_UNTRUSTED_ERROR"));
            assert!(row.asr_ms.is_some());
            if mode == "rejected" {
                assert_eq!(row.status, "rejected");
                assert_eq!(row.message, "guard-numbers");
                assert_eq!(row.candidate.as_deref(), Some("candidate fixture"));
            } else if mode == "unavailable" {
                assert_eq!(row.status, "unavailable");
            } else {
                assert_eq!(row.status, "error");
                assert!(row.correction_ms.is_some());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn serial_children_receive_only_the_selected_context_never_the_reference() {
        let root =
            std::env::temp_dir().join(format!("voice-comparison-routing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let paths = BTreeMap::from([
            ("base".to_owned(), Ok(root.join("speech.bin"))),
            ("qwen3-1.7b".to_owned(), Ok(root.join("correction.gguf"))),
        ]);
        let sources = crate::context::Sources {
            ax: "CONTEXT_FIXTURE".into(),
            ocr: "OCR_FIXTURE".into(),
        };
        for (source, context) in [
            ("ax", "CONTEXT_FIXTURE"),
            ("ocr", "OCR_FIXTURE"),
            ("both", "入力欄: CONTEXT_FIXTURE\n画面OCR: OCR_FIXTURE"),
        ] {
            for route in ["none", "asr-only", "correction-only", "both"] {
                let case = Case {
                    speech: "base".into(),
                    speech_backend: "cpu".into(),
                    correction: "qwen3-1.7b".into(),
                    correction_backend: "cpu".into(),
                    source: source.into(),
                    route: route.into(),
                };
                let expected_asr =
                    local_transcription::recognition_hint(if case.whisper_context() {
                        context
                    } else {
                        ""
                    });
                let expected_correction = if case.correction_context() {
                    context
                } else {
                    ""
                };
                let marker = root.join("asr-completed");
                let _ = std::fs::remove_file(&marker);
                let asr = format!(
                    "#!/usr/bin/python3\nimport sys,json,struct,os,time\nh=json.loads(sys.stdin.buffer.readline())\nb=sys.stdin.buffer.read()\nassert h=={{'samples':16000,'hint':{expected_asr:?}}}\nassert b==struct.pack('<16000f',*([0.0]*16000))\nassert b'REFERENCE_PRIVATE' not in b\nopen({marker:?},'w').write(str(os.getpid()))\ntime.sleep(0.05)\nprint(json.dumps(dict(text='raw fixture',backend='cpu',load_ms=1,elapsed_ms=2)))\n",
                    marker = marker.to_str().unwrap()
                );
                let correction = format!(
                    "#!/usr/bin/python3\nimport sys,json,os\np=sys.stdin.buffer.read()\nassert b'REFERENCE_PRIVATE' not in p\nassert json.loads(p)=={{'transcript':'raw fixture','context':{expected_correction:?}}}\nassert os.path.isfile({marker:?})\ntry:\n os.kill(int(open({marker:?}).read()),0)\nexcept ProcessLookupError:\n pass\nelse:\n raise AssertionError('previous helper was not reaped')\nprint(json.dumps(dict(candidate='raw fixture.',accepted=True,reason=None,backend='cpu',offloaded_layers=0)))\n",
                    marker = marker.to_str().unwrap()
                );
                for (name, source) in [
                    ("compare-transcription", asr),
                    ("local-correction", correction),
                ] {
                    let path = root.join(name);
                    std::fs::write(&path, source).unwrap();
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
                }
                let row = run_case(
                    &root,
                    &case,
                    0,
                    &vec![0.0; 16000],
                    &sources,
                    &paths,
                    &Ok(()),
                    Some("REFERENCE_PRIVATE"),
                    "",
                    &AtomicU64::new(1),
                    1,
                );
                assert_eq!(row.status, "accepted", "{route}: {}", row.message);
                assert_eq!(row.raw.as_deref(), Some("raw fixture"));
                assert_eq!(row.final_text.as_deref(), Some("raw fixture."));
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
