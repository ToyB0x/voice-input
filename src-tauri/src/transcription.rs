//! One owning worker retains the model; discard invalidates pending results.
use crate::recorder::Capture;
use local_transcription::{Backend, Transcriber, prepare_audio};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
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
    pub recognition_error: Option<String>,
    #[serde(skip)]
    generation: u64,
}

impl Session {
    pub fn clear_result(&mut self) {
        self.generation += 1;
        self.transcript = None;
        self.recognition_error = None;
        // In-flight native work cannot be safely killed. Keep controls locked
        // until it releases its PCM; its obsolete result will never be published.
        self.recognition_phase = if self.working { "discarding" } else { "idle" }.into();
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
            Ok(text) if !loading => self.transcript = Some(text),
            Ok(name) => {
                self.model_ready = true;
                self.model_name = name;
            }
            Err(error) => self.recognition_error = Some(error),
        }
    }
}

pub(crate) enum Job {
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
    },
}

pub(crate) fn load(
    session: &mut Session,
    path: String,
    backend: &str,
    tx: &mpsc::Sender<Job>,
) -> Result<(), String> {
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

pub(crate) fn recognize(capture: &mut Capture, tx: &mpsc::Sender<Job>) -> Result<(), String> {
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
    send(
        &mut capture.recognition,
        tx,
        Job::Recognize {
            audio,
            hint,
            rate,
            generation,
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
) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut model: Option<Transcriber> = None;
        for job in rx {
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
                Job::Suspend { .. } => unreachable!(),
            };
            let current = shared
                .lock()
                .is_ok_and(|c| c.recognition.generation == generation);
            let result = if !current {
                Err("破棄済み".into())
            } else {
                match job {
                    Job::Suspend { .. } => unreachable!(),
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
                        audio, rate, hint, ..
                    } => prepare_audio(&audio, rate)
                        .and_then(|pcm| {
                            let hint = hint.and_then(|hint| hint.take());
                            model
                                .as_ref()
                                .expect("model readiness is worker-owned")
                                .transcribe_with_hint(&pcm, hint.as_deref())
                        })
                        .map_err(|e| format!("文字起こしに失敗しました: {e}")),
                }
            };
            if let Ok(mut capture) = shared.lock() {
                if loading && capture.recognition.generation != generation {
                    model = None;
                }
                if !loading && let Some(ticket) = capture.insertion_ticket.take() {
                    if capture.recognition.generation == generation
                        && let Ok(text) = &result
                    {
                        // Serialize with discard: no detached or retried AX write.
                        inserter.apply(ticket, text);
                    } else {
                        inserter.cancel();
                    }
                }
                if !loading && !result.as_ref().is_ok_and(|text| !text.trim().is_empty()) {
                    capture.correction_hint = None;
                }
                capture.recognition.finish(generation, result, loading);
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
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
