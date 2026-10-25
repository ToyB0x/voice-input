use crate::audio_buffer::{AudioBuffer, MAX_SECONDS};
use crate::transcription::{self, Session};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
use serde::Serialize;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

fn permission_name(status: isize) -> &'static str {
    match status {
        0 => "not_determined",
        1 => "restricted",
        2 => "denied",
        3 => "authorized",
        _ => "unavailable",
    }
}

pub(crate) fn permission_status() -> &'static str {
    // AVMediaTypeAudio is the framework's constant. Supplying it is valid for
    // these class methods; no Objective-C object is shared across threads.
    unsafe {
        AVMediaTypeAudio.map_or("unavailable", |audio| {
            permission_name(AVCaptureDevice::authorizationStatusForMediaType(audio).0)
        })
    }
}

#[tauri::command]
pub async fn request_microphone_permission() -> Result<&'static str, String> {
    let status = permission_status();
    if status != "not_determined" {
        return Ok(status);
    }
    tauri::async_runtime::spawn_blocking(|| {
        let (tx, rx) = mpsc::channel();
        let callback = block2::RcBlock::new(move |_: objc2::runtime::Bool| {
            let _ = tx.send(());
        });
        unsafe {
            let audio = AVMediaTypeAudio.ok_or("マイク権限を確認できません。")?;
            AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &callback);
        }
        rx.recv_timeout(Duration::from_secs(120)).map_err(|_| {
            "マイクの許可待ちがタイムアウトしました。権限を確認して再試行してください。".to_string()
        })?;
        Ok(permission_status())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub(crate) phase: &'static str,
    permission: &'static str,
    elapsed_ms: u64,
    level: f32,
    pub(crate) sample_count: usize,
    sample_rate: u32,
    error: Option<String>,
    #[serde(flatten)]
    pub(crate) recognition: Session,
}

#[derive(Default)]
pub(crate) struct Capture {
    pub(crate) audio: Option<AudioBuffer>,
    phase: Option<&'static str>,
    error: Option<String>,
    pub(crate) recognition: Session,
    pub(crate) insertion_ticket: Option<u64>,
    pub(crate) context_hint: Option<crate::context::Hint>,
    pub(crate) correction_hint: Option<crate::context::Hint>,
    comparison_hint: Option<crate::context::Hint>,
}

impl Capture {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase.unwrap_or("idle"),
            permission: permission_status(),
            elapsed_ms: self.audio.as_ref().map_or(0, AudioBuffer::elapsed_ms),
            level: if self.phase == Some("recording") {
                self.audio.as_ref().map_or(0.0, |a| a.level)
            } else {
                0.0
            },
            sample_count: self.audio.as_ref().map_or(0, |a| a.samples.len()),
            sample_rate: self.audio.as_ref().map_or(0, |a| a.sample_rate),
            error: self.error.clone(),
            recognition: self.recognition.clone(),
        }
    }

    fn fail(&mut self, error: String) {
        self.phase = Some("error");
        self.error = Some(error);
    }

    fn finish(&mut self) {
        if self.error.is_none() {
            if self.audio.as_ref().is_some_and(|a| !a.samples.is_empty()) {
                self.phase = Some("recorded");
            } else {
                self.fail("音声を取得できませんでした。入力デバイスを確認してください。".into());
            }
        }
    }
}

enum Action {
    Start,
    Toggle,
    Stop,
    Clear,
    LoadModel(String, String),
    Transcribe,
}
struct Request {
    action: Action,
    reply: mpsc::Sender<Result<Snapshot, String>>,
}

pub(crate) struct ComparisonInput {
    pub audio: Vec<f32>,
    pub rate: u32,
    pub hint: Option<crate::context::Hint>,
}

#[derive(Clone)]
pub struct Recorder {
    requests: mpsc::Sender<Request>,
    capture: Arc<Mutex<Capture>>,
    jobs: mpsc::Sender<transcription::Job>,
}

impl Recorder {
    pub fn new(
        inserter: crate::insertion::Inserter,
        context: crate::context::Reader,
        corrector: crate::correction::Corrector,
        comparisons: crate::comparison::Comparisons,
        updates: crate::updater::Updates,
        activity: crate::activity::Activity,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let capture = Arc::new(Mutex::new(Capture::default()));
        let jobs = transcription::worker(capture.clone(), inserter.clone());
        let shared = capture.clone();
        let comparison_jobs = jobs.clone();
        std::thread::spawn(move || {
            // The stream stays on its owning thread. Audio callbacks only fill
            // bounded, preallocated memory; IPC never serializes audio samples.
            let mut stream: Option<(cpal::Stream, Instant)> = None;
            loop {
                let should_stop = stream.as_ref().is_some_and(|(_, started)| {
                    started.elapsed().as_secs() >= u64::from(MAX_SECONDS)
                        || shared.lock().map_or(true, |c| {
                            c.error.is_some() || c.audio.as_ref().is_some_and(AudioBuffer::is_full)
                        })
                });
                if should_stop {
                    drop(stream.take());
                    if let Ok(mut c) = shared.lock() {
                        c.finish();
                        if c.phase == Some("recorded") && c.recognition.model_ready {
                            let _ = transcription::recognize(&mut c, &jobs);
                        } else if c.phase == Some("error") {
                            context.cancel();
                            c.context_hint = None;
                            c.correction_hint = None;
                        }
                    }
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(request) => {
                        let action = match request.action {
                            Action::Toggle if stream.is_some() => Action::Stop,
                            Action::Toggle => Action::Start,
                            other => other,
                        };
                        let gate = activity.lock();
                        if gate.is_err() {
                            let _ = request.reply.send(Err("処理状態を確認できません。".into()));
                            continue;
                        }
                        let _gate = gate.unwrap();
                        if !matches!(&action, Action::Clear)
                            && (comparisons.working() || updates.installing())
                        {
                            let _ = request
                                .reply
                                .send(Err("比較または更新の終了をお待ちください。".into()));
                            continue;
                        }
                        let result = match action {
                            Action::Toggle => {
                                unreachable!("toggle is resolved on the stream-owning thread")
                            }
                            Action::Start if shared.lock().is_ok_and(|c| c.recognition.working) => {
                                Err("処理が終わるまでお待ちください。".into())
                            }
                            Action::Start if stream.is_some() => Err("すでに録音中です。".into()),
                            Action::Start => {
                                // Invalidate and hide the old transcript under the same
                                // lock used to acquire correction input, before slow AX/OCR.
                                {
                                    let mut c =
                                        shared.lock().unwrap_or_else(|error| error.into_inner());
                                    corrector.cancel();
                                    c.recognition.clear_result();
                                    c.phase = Some("preparing");
                                    c.context_hint = None;
                                    c.correction_hint = None;
                                }
                                comparisons.cancel(true);
                                inserter.cancel();
                                let ticket = inserter.begin();
                                context.cancel();
                                let hint = context.capture();
                                let comparison_hint = if comparisons.armed() {
                                    hint.clone()
                                } else {
                                    None
                                };
                                let correction_hint = if context.retain_for_correction() {
                                    hint.clone()
                                } else {
                                    None
                                };
                                if let Ok(mut c) = shared.lock() {
                                    let mut recognition = std::mem::take(&mut c.recognition);
                                    recognition.clear_result();
                                    *c = Capture {
                                        recognition,
                                        insertion_ticket: ticket,
                                        context_hint: hint,
                                        correction_hint,
                                        comparison_hint,
                                        ..Capture::default()
                                    };
                                }
                                match start_stream(shared.clone()) {
                                    Ok(input) => {
                                        stream = Some((input, Instant::now()));
                                        Ok(())
                                    }
                                    Err(error) => {
                                        inserter.cancel();
                                        context.cancel();
                                        if let Ok(mut c) = shared.lock() {
                                            c.insertion_ticket = None;
                                            c.context_hint = None;
                                            c.correction_hint = None;
                                            c.fail(error.clone());
                                        }
                                        Err(error)
                                    }
                                }
                            }
                            Action::Stop => {
                                let was_recording = stream.take();
                                if was_recording.is_some() {
                                    drop(was_recording);
                                    if let Ok(mut c) = shared.lock() {
                                        c.finish();
                                        if c.phase == Some("recorded") && c.recognition.model_ready
                                        {
                                            let _ = transcription::recognize(&mut c, &jobs);
                                        } else if c.phase == Some("error") {
                                            context.cancel();
                                            c.context_hint = None;
                                            c.correction_hint = None;
                                        }
                                    }
                                }
                                Ok(())
                            }
                            Action::LoadModel(_, _) | Action::Transcribe if stream.is_some() => {
                                Err("録音を停止してから操作してください。".into())
                            }
                            Action::LoadModel(path, backend) => {
                                shared.lock().map_err(|e| e.to_string()).and_then(|mut c| {
                                    corrector.cancel();
                                    transcription::load(&mut c.recognition, path, &backend, &jobs)
                                })
                            }
                            Action::Transcribe => {
                                shared.lock().map_err(|e| e.to_string()).and_then(|mut c| {
                                    corrector.cancel();
                                    transcription::recognize(&mut c, &jobs)
                                })
                            }
                            Action::Clear => {
                                comparisons.cancel(true);
                                inserter.cancel();
                                context.cancel();
                                drop(stream.take());
                                if let Ok(mut c) = shared.lock() {
                                    corrector.cancel();
                                    c.audio = None;
                                    c.comparison_hint = None;
                                    c.insertion_ticket = None;
                                    c.context_hint = None;
                                    c.correction_hint = None;
                                    c.phase = None;
                                    c.error = None;
                                    c.recognition.clear_result();
                                }
                                Ok(())
                            }
                        };
                        let reply = result.and_then(|()| {
                            shared
                                .lock()
                                .map(|c| c.snapshot())
                                .map_err(|e| e.to_string())
                        });
                        let _ = request.reply.send(reply);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self {
            requests: tx,
            capture,
            jobs: comparison_jobs,
        }
    }

    pub(crate) fn comparison_input(&self) -> Result<ComparisonInput, String> {
        let capture = self
            .capture
            .lock()
            .map_err(|_| "録音状態を確認できません。")?;
        if capture.recognition.working || matches!(capture.phase, Some("recording" | "preparing")) {
            return Err("録音と文字起こしの終了をお待ちください。".into());
        }
        let audio = capture
            .audio
            .as_ref()
            .filter(|a| !a.samples.is_empty())
            .ok_or("先に比較する音声を録音してください。")?;
        Ok(ComparisonInput {
            audio: audio.samples.clone(),
            rate: audio.sample_rate,
            hint: capture.comparison_hint.clone(),
        })
    }
    pub(crate) fn clear_comparison_hint(&self) {
        if let Ok(mut capture) = self.capture.lock() {
            capture.comparison_hint = None;
        }
    }
    pub(crate) fn suspend_for_comparison(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(transcription::Job::Suspend { reply: tx })
            .map_err(|_| "音声ワーカーを確認できません。")?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "音声モデルの解放を確認できません。".into())
    }
    pub(crate) fn correction_input(
        &self,
    ) -> Result<(String, Option<crate::context::Hint>), String> {
        let mut capture = self
            .capture
            .lock()
            .map_err(|_| "録音状態を確認できません。")?;
        if capture.recognition.working || matches!(capture.phase, Some("recording" | "preparing")) {
            return Err("録音と文字起こしの終了をお待ちください。".into());
        }
        let text = capture
            .recognition
            .transcript
            .clone()
            .filter(|text| !text.trim().is_empty())
            .ok_or("先に音声を文字起こししてください。")?;
        Ok((text, capture.correction_hint.take()))
    }
    pub(crate) fn status(&self) -> Result<Snapshot, String> {
        self.capture
            .lock()
            .map(|c| c.snapshot())
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn toggle(&self) -> Result<Snapshot, String> {
        self.send(Action::Toggle).await
    }

    pub(crate) async fn restore_model(
        &self,
        path: String,
        backend: String,
    ) -> Result<Snapshot, String> {
        self.send(Action::LoadModel(path, backend)).await
    }

    async fn send(&self, action: Action) -> Result<Snapshot, String> {
        let requests = self.requests.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let (reply, rx) = mpsc::channel();
            requests
                .send(Request { action, reply })
                .map_err(|e| e.to_string())?;
            rx.recv_timeout(Duration::from_secs(10))
                .map_err(|e| e.to_string())?
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    capture: Arc<Mutex<Capture>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let errors = capture.clone();
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data, _| {
                if let Ok(mut c) = capture.lock()
                    && c.phase == Some("recording")
                    && let Some(audio) = c.audio.as_mut()
                {
                    audio.push(data.iter().map(|&s| <f32 as cpal::Sample>::from_sample(s)));
                }
            },
            move |error| {
                if let Ok(mut c) = errors.lock() {
                    c.fail(format!("録音デバイスでエラーが発生しました: {error}"));
                }
            },
            None,
        )
        .map_err(|e| format!("入力デバイスを開始できません: {e}"))
}

fn start_stream(capture: Arc<Mutex<Capture>>) -> Result<cpal::Stream, String> {
    if permission_status() != "authorized" {
        return Err("マイクへのアクセスが許可されていません。".into());
    }
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("入力デバイスが見つかりません。マイクを接続してください。")?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("入力形式を確認できません: {e}"))?;
    let audio = AudioBuffer::new(supported.sample_rate(), usize::from(supported.channels()))?;
    {
        let mut c = capture.lock().map_err(|e| e.to_string())?;
        c.audio = Some(audio);
        c.phase = Some("recording");
    }
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, config, capture)?,
        cpal::SampleFormat::F64 => build_stream::<f64>(&device, config, capture)?,
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, config, capture)?,
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, config, capture)?,
        format => return Err(format!("対応していない音声形式です: {format}")),
    };
    stream
        .play()
        .map_err(|e| format!("録音を開始できません: {e}"))?;
    Ok(stream)
}

#[tauri::command]
pub fn recording_status(recorder: tauri::State<'_, Recorder>) -> Result<Snapshot, String> {
    recorder
        .capture
        .lock()
        .map(|c| c.snapshot())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_recording(recorder: tauri::State<'_, Recorder>) -> Result<Snapshot, String> {
    recorder.send(Action::Start).await
}
#[tauri::command]
pub async fn stop_recording(recorder: tauri::State<'_, Recorder>) -> Result<Snapshot, String> {
    recorder.send(Action::Stop).await
}
#[tauri::command]
pub async fn clear_recording(recorder: tauri::State<'_, Recorder>) -> Result<Snapshot, String> {
    recorder.send(Action::Clear).await
}

#[tauri::command]
pub fn open_microphone_settings() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .status()
        .map_err(|e| e.to_string())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("システム設定を開けませんでした。".into())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_states_preserve_denied_and_restricted() {
        assert_eq!(permission_name(0), "not_determined");
        assert_eq!(permission_name(1), "restricted");
        assert_eq!(permission_name(2), "denied");
        assert_eq!(permission_name(3), "authorized");
        assert_eq!(permission_name(99), "unavailable");
    }
    #[test]
    fn empty_capture_and_failed_stream_are_not_reported_as_recorded() {
        let mut capture = Capture::default();
        capture.finish();
        assert_eq!(capture.snapshot().phase, "error");
        capture.fail("device disconnected".into());
        capture.finish();
        assert_eq!(
            capture.snapshot().error.as_deref(),
            Some("device disconnected")
        );
    }
    #[test]
    fn recorded_samples_are_retained_until_cleared_and_level_resets() {
        let mut audio = AudioBuffer::new(8_000, 1).unwrap();
        audio.push([0.5; 800].into_iter());
        let mut capture = Capture {
            audio: Some(audio),
            phase: Some("recording"),
            error: None,
            recognition: Session::default(),
            insertion_ticket: None,
            context_hint: None,
            correction_hint: None,
            comparison_hint: None,
        };
        capture.finish();
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.phase, "recorded");
        assert_eq!(snapshot.sample_count, 800);
        assert_eq!(snapshot.elapsed_ms, 100);
        assert_eq!(snapshot.level, 0.0);
        capture = Capture::default();
        assert_eq!(capture.snapshot().sample_count, 0);
    }
}

#[tauri::command]
pub async fn load_model(
    recorder: tauri::State<'_, Recorder>,
    path: String,
    backend: String,
) -> Result<Snapshot, String> {
    recorder.send(Action::LoadModel(path, backend)).await
}

#[tauri::command]
pub async fn transcribe_recording(
    recorder: tauri::State<'_, Recorder>,
) -> Result<Snapshot, String> {
    recorder.send(Action::Transcribe).await
}

#[tauri::command]
pub fn copy_transcript(recorder: tauri::State<'_, Recorder>) -> Result<(), String> {
    let capture = recorder.capture.lock().map_err(|e| e.to_string())?;
    if capture.recognition.working {
        return Err("処理が終わるまでお待ちください。".into());
    }
    let text = capture
        .recognition
        .transcript
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or("コピーする認識結果がありません。")?;
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|e| format!("コピーできませんでした: {e}"))
}
