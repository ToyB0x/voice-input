//! One owning worker retains the model; discard invalidates pending results.
use crate::recorder::Capture;
use local_transcription::{Backend, Transcriber, prepare_audio};
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    pub model_ready: bool,
    pub model_path: String,
    pub model_name: String,
    pub model_bytes: u64,
    pub backend: String,
    pub working: bool,
    pub recognition_phase: String,
    pub transcript: Option<String>,
    pub final_text: Option<String>,
    pub final_source: String,
    pub recognition_error: Option<String>,
    #[serde(skip)]
    generation: u64,
}

impl Session {
    pub fn clear_result(&mut self) {
        self.generation += 1;
        self.transcript = None;
        self.final_text = None;
        self.final_source.clear();
        self.recognition_error = None;
        // In-flight native work cannot be safely killed. Keep controls locked
        // until it releases its PCM; its obsolete result will never be published.
        self.recognition_phase = if self.working { "discarding" } else { "idle" }.into();
    }
    pub(crate) fn cancel_automatic(&mut self) {
        if self.final_source == "correcting" {
            self.final_source = "cancelled".into();
            self.final_text = self.transcript.clone();
        }
    }
    // Called while holding Capture, the same lock used by discard/new recording.
    fn complete_automatic(
        &mut self,
        generation: u64,
        raw: &str,
        result: Result<String, String>,
        deliver: impl FnOnce(&str),
    ) {
        if generation != self.generation || self.final_source != "correcting" {
            return;
        }
        let text = match result {
            Ok(text) => {
                self.final_source = "correction".into();
                text
            }
            Err(_) => {
                self.final_source = "fallback".into();
                raw.into()
            }
        };
        self.final_text = Some(text.clone());
        deliver(&text);
    }
    fn begin(&mut self, phase: &str) -> u64 {
        self.clear_result();
        self.working = true;
        self.recognition_phase = phase.into();
        self.generation
    }
    fn finish(&mut self, generation: u64, result: Result<String, String>, loading: bool) {
        self.working = false;
        self.recognition_phase = "idle".into();
        if generation != self.generation {
            return;
        }
        match result {
            Ok(text) if !loading => {
                self.final_text = Some(text.clone());
                self.final_source = "whisper".into();
                self.transcript = Some(text);
            }
            Ok(name) => {
                self.model_ready = true;
                self.model_name = name;
            }
            Err(error) => self.recognition_error = Some(error),
        }
    }
}

pub(crate) struct AutomaticRequest {
    config: crate::correction::AutomaticConfig,
    generation: u64,
    hint: Option<crate::context::Hint>,
}
pub(crate) enum Job {
    Shutdown,
    Suspend {
        reply: mpsc::Sender<()>,
    },
    Load {
        path: String,
        backend: Backend,
        generation: u64,
    },
    Recognize {
        audio: Vec<f32>,
        hint: Option<crate::context::Hint>,
        rate: u32,
        generation: u64,
        automatic: Option<AutomaticRequest>,
    },
}

pub(crate) fn load(
    session: &mut Session,
    path: String,
    backend: &str,
    tx: &mpsc::Sender<Job>,
) -> Result<(), String> {
    // A replacement request may fail validation before begin() clears the session.
    session.cancel_automatic();
    if session.working {
        return Err("処理が終わるまでお待ちください。".into());
    }
    let selected = match backend {
        "metal" => Backend::Metal,
        "cpu" => Backend::Cpu,
        _ => return Err("実行方式を選択してください。".into()),
    };
    if path.trim().is_empty() || path.len() > 4096 {
        return Err("モデルの絶対パスを指定してください。".into());
    }
    let path = path.trim().to_owned();
    if !Path::new(&path).is_absolute() {
        return Err("モデルの絶対パスを指定してください。".into());
    }
    session.model_ready = false;
    session.model_name.clear();
    session.model_bytes = std::fs::metadata(&path).map_or(0, |file| file.len());
    session.model_path = path.clone();
    session.backend = backend.into();
    let generation = session.begin("loading");
    send(
        session,
        tx,
        Job::Load {
            path,
            backend: selected,
            generation,
        },
    )
}

pub(crate) fn recognize(
    capture: &mut Capture,
    tx: &mpsc::Sender<Job>,
    config: Option<crate::correction::AutomaticConfig>,
    corrector: &crate::correction::Corrector,
) -> Result<(), String> {
    capture.recognition.cancel_automatic();
    if capture.recognition.working {
        return Err("処理が終わるまでお待ちください。".into());
    }
    if !capture.recognition.model_ready {
        return Err("先にローカルモデルを読み込んでください。".into());
    }
    let audio = capture
        .audio
        .as_ref()
        .filter(|a| !a.samples.is_empty())
        .ok_or("先に音声を録音してください。")?;
    let (audio, rate) = (audio.samples.clone(), audio.sample_rate);
    let generation = capture.recognition.begin("transcribing");
    let hint = capture.context_hint.take();
    let automatic = config.map(|config| AutomaticRequest {
        config,
        generation: corrector.generation(),
        hint: capture.correction_hint.take(),
    });
    send(
        &mut capture.recognition,
        tx,
        Job::Recognize {
            audio,
            hint,
            rate,
            generation,
            automatic,
        },
    )
}

fn send(session: &mut Session, tx: &mpsc::Sender<Job>, job: Job) -> Result<(), String> {
    if tx.send(job).is_err() {
        session.finish(
            session.generation,
            Err("文字起こしワーカーが停止しました。アプリを再起動してください。".into()),
            false,
        );
        return Err("文字起こしワーカーが停止しました。".into());
    }
    Ok(())
}

pub(crate) fn worker(
    shared: Arc<Mutex<Capture>>,
    inserter: crate::insertion::Inserter,
    closing: Arc<AtomicBool>,
    corrector: crate::correction::Corrector,
) -> (mpsc::Sender<Job>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut model: Option<Transcriber> = None;
        for job in rx {
            if closing.load(Ordering::Acquire) || matches!(job, Job::Shutdown) {
                break;
            }
            if let Job::Suspend { reply } = job {
                model = None;
                if let Ok(mut capture) = shared.lock() {
                    capture.recognition.model_ready = false;
                    capture.recognition.model_name.clear();
                }
                let _ = reply.send(());
                continue;
            }
            let (generation, loading) = match &job {
                Job::Load { generation, .. } => (*generation, true),
                Job::Recognize { generation, .. } => (*generation, false),
                Job::Suspend { .. } | Job::Shutdown => unreachable!(),
            };
            let current = shared
                .lock()
                .is_ok_and(|c| c.recognition.generation == generation);
            let mut automatic = None;
            let result = if !current {
                Err("破棄済み".into())
            } else {
                match job {
                    Job::Suspend { .. } | Job::Shutdown => unreachable!(),
                    Job::Load { path, backend, .. } => {
                        // Drop the old model first to bound peak memory on replacement.
                        model = None;
                        match Transcriber::load(Path::new(&path), backend) {
                            Ok(engine) => {
                                let name = engine.model_name();
                                model = Some(engine);
                                crate::lifecycle::diagnostic("model-ready");
                                Ok(name)
                            }
                            Err(e) => Err(format!(
                                "モデルを読み込めません。配置と形式を確認してください: {e}"
                            )),
                        }
                    }
                    Job::Recognize {
                        audio,
                        rate,
                        hint,
                        automatic: request,
                        ..
                    } => {
                        automatic = request;
                        prepare_audio(&audio, rate)
                            .and_then(|pcm| {
                                let hint = hint.and_then(|hint| hint.take());
                                model
                                    .as_ref()
                                    .expect("model readiness is worker-owned")
                                    .transcribe_with_hint(&pcm, hint.as_deref())
                            })
                            .map_err(|e| format!("文字起こしに失敗しました: {e}"))
                    }
                }
            };
            if loading
                && shared
                    .lock()
                    .is_ok_and(|c| c.recognition.generation != generation)
            {
                model = None;
            }
            finish_recognition(
                &shared, &inserter, &corrector, &closing, generation, result, loading, automatic,
            );
        }
    });
    (tx, handle)
}

#[allow(clippy::too_many_arguments)]
fn finish_recognition(
    shared: &Arc<Mutex<Capture>>,
    inserter: &crate::insertion::Inserter,
    corrector: &crate::correction::Corrector,
    closing: &Arc<AtomicBool>,
    generation: u64,
    result: Result<String, String>,
    loading: bool,
    automatic: Option<AutomaticRequest>,
) {
    let Ok(mut capture) = shared.lock() else {
        return;
    };
    let current = capture.recognition.generation == generation && !closing.load(Ordering::Acquire);
    let ticket = if !loading {
        capture.insertion_ticket.take()
    } else {
        None
    };
    if !loading && !result.as_ref().is_ok_and(|text| !text.trim().is_empty()) {
        capture.correction_hint = None;
    }
    capture
        .recognition
        .finish(generation, result.clone(), loading);
    if !current || loading {
        return;
    }
    let Ok(raw) = result else {
        inserter.cancel();
        return;
    };
    if let Some(request) = automatic.filter(|_| !raw.trim().is_empty()) {
        capture.recognition.final_text = None;
        capture.recognition.final_source = "correcting".into();
        let target = shared.clone();
        let delivery = inserter.clone();
        let stopping = closing.clone();
        let fallback = raw.clone();
        let validity = corrector.clone();
        let expected = request.generation.wrapping_add(1);
        if let Err(error) = corrector.start_automatic(
            request.config,
            raw,
            request.hint,
            request.generation,
            move |result| {
                if let Ok(mut capture) = target.lock()
                    && !stopping.load(Ordering::Acquire)
                    && validity.generation() == expected
                {
                    capture
                        .recognition
                        .complete_automatic(generation, &fallback, result, |text| {
                            if let Some(ticket) = ticket {
                                delivery.apply(ticket, text);
                            }
                        });
                }
            },
        ) {
            capture.recognition.cancel_automatic();
            capture.recognition.recognition_error = Some(error);
            inserter.cancel();
        }
    } else if let Some(ticket) = ticket {
        inserter.apply(ticket, &raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_delivery_is_once_and_invalidations_never_fall_back_to_insertion() {
        for success in [true, false] {
            let mut session = Session::default();
            let generation = session.begin("transcribing");
            session.finish(generation, Ok("raw".into()), false);
            session.final_source = "correcting".into();
            session.final_text = None;
            let mut writes = Vec::new();
            session.complete_automatic(
                generation,
                "raw",
                if success {
                    Ok("corrected".into())
                } else {
                    Err("guard rejected".into())
                },
                |text| writes.push(text.to_owned()),
            );
            session.complete_automatic(generation, "raw", Ok("duplicate".into()), |text| {
                writes.push(text.to_owned())
            });
            assert_eq!(writes, vec![if success { "corrected" } else { "raw" }]);
            assert_eq!(session.transcript.as_deref(), Some("raw"));
        }
        for discard in [true, false] {
            let mut session = Session::default();
            let generation = session.begin("transcribing");
            session.finish(generation, Ok("raw".into()), false);
            session.final_source = "correcting".into();
            if discard {
                session.clear_result();
            } else {
                session.cancel_automatic();
            }
            session.complete_automatic(generation, "raw", Err("late failure".into()), |_| {
                panic!("cancelled result was inserted")
            });
            assert_ne!(session.final_source, "fallback");
        }
    }
    #[test]
    fn automatic_worker_waits_preserves_raw_and_cancels_delayed_helpers() {
        use std::{
            os::unix::fs::PermissionsExt,
            time::{Duration, Instant},
        };
        let root =
            std::env::temp_dir().join(format!("voice-automatic-pipeline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let runtime = root.join("local-correction");
        let started = root.join("started");
        let release = root.join("release");
        let script = format!(
            r#"#!/usr/bin/python3
import json,sys,time,os
from pathlib import Path
p=json.load(sys.stdin)
assert p['context']==''
Path({started:?}).write_text(str(os.getpid()))
while not Path({release:?}).exists():time.sleep(0.01)
if p['transcript']=='reject':
 print(json.dumps(dict(error='rejected')))
 sys.exit(1)
print(json.dumps(dict(text=p['transcript']+'。',model='fixture',backend='cpu',offloaded_layers=0,elapsed_ms=1)))
"#,
            started = started.to_str().unwrap(),
            release = release.to_str().unwrap()
        );
        std::fs::write(&runtime, script).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = crate::correction::AutomaticConfig {
            runtime,
            model: root.join("model.gguf").to_string_lossy().into(),
            backend: "cpu".into(),
        };
        let corrector = crate::correction::Corrector::new();
        let inserter = crate::insertion::Inserter::new();
        let closing = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Capture::default()));
        let wait = |ready: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !ready() && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(ready());
        };
        for (raw, mode) in [
            ("accepted", "success"),
            ("reject", "fallback"),
            ("old", "discard"),
            ("settings", "settings"),
            ("exit", "exit"),
        ] {
            let _ = std::fs::remove_file(&started);
            let _ = std::fs::remove_file(&release);
            let generation = shared.lock().unwrap().recognition.begin("transcribing");
            let request = AutomaticRequest {
                config: config.clone(),
                generation: corrector.generation(),
                hint: None,
            };
            finish_recognition(
                &shared,
                &inserter,
                &corrector,
                &closing,
                generation,
                Ok(raw.into()),
                false,
                Some(request),
            );
            wait(&|| {
                std::fs::read_to_string(&started)
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
                    .is_some()
            });
            {
                let capture = shared.lock().unwrap();
                assert_eq!(capture.recognition.transcript.as_deref(), Some(raw));
                assert!(capture.recognition.final_text.is_none());
            }
            if mode == "discard" {
                let mut c = shared.lock().unwrap();
                corrector.cancel();
                c.recognition.clear_result();
            }
            if mode == "settings" {
                let mut c = shared.lock().unwrap();
                corrector.cancel();
                c.recognition.cancel_automatic();
            }
            if mode == "exit" {
                closing.store(true, Ordering::Release);
                corrector.cancel();
            }
            std::fs::write(&release, "ready").unwrap();
            wait(&|| !corrector.status().working);
            let capture = shared.lock().unwrap();
            match mode {
                "success" => {
                    assert_eq!(
                        capture.recognition.final_text.as_deref(),
                        Some("accepted。")
                    );
                    assert_eq!(capture.recognition.final_source, "correction");
                }
                "fallback" => {
                    assert_eq!(capture.recognition.final_text.as_deref(), Some("reject"));
                    assert_eq!(capture.recognition.final_source, "fallback");
                }
                "discard" => assert!(
                    capture.recognition.transcript.is_none()
                        && capture.recognition.final_text.is_none()
                ),
                "settings" => assert_eq!(capture.recognition.final_source, "cancelled"),
                _ => assert!(capture.recognition.final_text.is_none()),
            }
        }
        // A settings change during ASR invalidates the queued correction before
        // it can reserve a worker. It must not insert a raw fallback either.
        closing.store(false, Ordering::Release);
        std::fs::remove_file(&started).unwrap();
        let generation = shared.lock().unwrap().recognition.begin("transcribing");
        let request = AutomaticRequest {
            config: config.clone(),
            generation: corrector.generation(),
            hint: None,
        };
        corrector.cancel();
        finish_recognition(
            &shared,
            &inserter,
            &corrector,
            &closing,
            generation,
            Ok("stale settings".into()),
            false,
            Some(request),
        );
        {
            let capture = shared.lock().unwrap();
            assert_eq!(capture.recognition.final_source, "cancelled");
            assert_eq!(
                capture.recognition.final_text.as_deref(),
                Some("stale settings")
            );
        }
        assert!(!started.exists());
        let generation = shared.lock().unwrap().recognition.begin("transcribing");
        finish_recognition(
            &shared,
            &inserter,
            &corrector,
            &closing,
            generation,
            Ok("plain ASR".into()),
            false,
            None,
        );
        assert_eq!(shared.lock().unwrap().recognition.final_source, "whisper");
        corrector.shutdown();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_replacement_requests_finalize_cancelled_automatic_results() {
        let (tx, _rx) = mpsc::channel();
        for (path, backend) in [("relative-model.bin", "cpu"), ("/tmp/model.bin", "invalid")] {
            let mut capture = Capture::default();
            let generation = capture.recognition.begin("transcribing");
            capture
                .recognition
                .finish(generation, Ok("retained raw".into()), false);
            capture.recognition.final_source = "correcting".into();
            capture.recognition.final_text = None;
            assert!(load(&mut capture.recognition, path.into(), backend, &tx).is_err());
            assert_eq!(capture.recognition.final_source, "cancelled");
            assert_eq!(
                capture.recognition.final_text.as_deref(),
                Some("retained raw")
            );
            capture.recognition.complete_automatic(
                generation,
                "retained raw",
                Ok("late".into()),
                |_| panic!("invalid replacement allowed old delivery"),
            );
        }
        let corrector = crate::correction::Corrector::new();
        let mut capture = Capture::default();
        let generation = capture.recognition.begin("transcribing");
        capture
            .recognition
            .finish(generation, Ok("retained raw".into()), false);
        capture.recognition.final_source = "correcting".into();
        capture.recognition.final_text = None;
        assert!(recognize(&mut capture, &tx, None, &corrector).is_err());
        assert_eq!(capture.recognition.final_source, "cancelled");
        assert_eq!(
            capture.recognition.final_text.as_deref(),
            Some("retained raw")
        );
    }
    #[test]
    fn discard_drops_stale_text_and_locks_until_worker_finishes() {
        let mut session = Session {
            model_ready: true,
            ..Session::default()
        };
        let job = session.begin("transcribing");
        session.clear_result();
        assert!(session.working);
        assert_eq!(session.recognition_phase, "discarding");
        session.finish(job, Ok("obsolete private text".into()), false);
        assert!(!session.working);
        assert!(session.transcript.is_none());
        assert!(session.model_ready);
        let next = session.begin("transcribing");
        session.finish(next, Ok("current text".into()), false);
        assert_eq!(session.transcript.as_deref(), Some("current text"));
    }
    #[test]
    fn failed_model_and_stale_load_do_not_report_ready() {
        let mut session = Session::default();
        let generation = session.begin("loading");
        session.finish(generation, Err("missing".into()), true);
        assert!(!session.model_ready);
        assert!(session.recognition_error.is_some());
        let generation = session.begin("loading");
        session.clear_result();
        session.finish(generation, Ok(String::new()), true);
        assert!(!session.model_ready);
    }
}
