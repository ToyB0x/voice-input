//! The three-stage LT demo shares comparison exclusion, cancellation, and quit cleanup.
use super::*;
use crate::demo_protocol as protocol;
use serde_json::{Value, json};
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Session {
    pub(super) raw: Option<String>,
    resident: bool,
    backend: Option<String>,
    pub(super) rows: Vec<Value>,
}
impl Session {
    fn prepare(&mut self, options: &Options) {
        if options.stage == "release" {
            return;
        }
        if options.stage == "audio"
            && options.resident
            && self.resident
            && self.backend.as_deref() == Some(options.backend())
        {
            self.raw = None;
            self.rows.retain(|row| row["stage"] == "prepare");
        } else if options.stage == "audio"
            || options.stage == "prepare"
            || self.resident != options.resident
            || self.backend.as_deref() != Some(options.backend())
        {
            *self = Self::default();
        }
        self.backend = Some(options.backend().into());
        self.resident = options.resident;
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default)]
    pub(super) use_gpu: bool,
    #[serde(default)]
    pub(super) resident: bool,
    pub(super) stage: String,
    pub(super) repeats: usize,
    pub(super) texts: Vec<String>,
}
impl Options {
    pub(super) fn backend(&self) -> &'static str {
        if self.use_gpu { "metal" } else { "cpu" }
    }
    fn validate(&self) -> Result<(), String> {
        if !["audio", "text", "image", "prepare", "release"].contains(&self.stage.as_str())
            || (["prepare", "release"].contains(&self.stage.as_str()) && !self.resident)
            || !(1..=2).contains(&self.repeats)
            || self.texts.len() != 2
            || self
                .texts
                .iter()
                .any(|s| s.chars().count() > 256 || s.contains('\0'))
            || (self.stage == "text" && self.texts.iter().any(|s| s.trim().is_empty()))
        {
            return Err("デモの段階・反復回数・文章を確認してください。".into());
        }
        Ok(())
    }
}
#[tauri::command]
pub fn demo_status(comparisons: tauri::State<'_, Comparisons>) -> Value {
    let s = comparisons.inner.lock().unwrap_or_else(|e| e.into_inner());
    json!({"working":s.status.working,"mode":s.status.mode,"phase":s.status.phase,"message":s.status.message,
        "generation":s.status.generation,"completed":s.status.completed,"total":s.status.total,
        "residentReadyCpu":comparisons.resident.ready("cpu"),"residentReadyMetal":comparisons.resident.ready("metal"),"raw":s.demo.raw,"rows":s.demo.rows,"backend":s.demo.backend,"ocr":false,"asrHint":"",
        "model":serde_json::from_str::<Value>(protocol::MODEL).unwrap(),"networkDenied":false,"sourceRevision":env!("VOICE_INPUT_SOURCE_REVISION")})
}
#[tauri::command]
pub fn clear_demo(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
) -> Result<(), String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    let mut s = comparisons
        .inner
        .lock()
        .map_err(|_| "状態を確認できません。")?;
    if s.status.working {
        return Err("実行中の処理の終了をお待ちください。".into());
    }
    comparisons.resident.release();
    s.demo = Session::default();
    if s.status.mode == "demo" {
        s.status.phase = "idle".into();
        s.status.message = "デモ結果を消去しました。".into();
        s.status.completed = 0;
        s.status.total = 0;
    }
    Ok(())
}
#[tauri::command]
pub fn start_demo(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    options: Options,
) -> Result<(), String> {
    options.validate()?;
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    ensure_comparison_available(&app)?;
    recorder.ensure_comparison_idle()?;
    let mut handle = comparisons
        .worker
        .lock()
        .map_err(|_| "ワーカーを確認できません。")?;
    if comparisons.closing.load(Ordering::Acquire) || comparisons.working() {
        return Err("実行中の処理の終了をお待ちください。".into());
    }
    if let Some(old) = handle.take() {
        let _ = old.join();
    }
    let epoch = comparisons.epoch.fetch_add(1, Ordering::AcqRel) + 1;
    {
        let mut s = comparisons
            .inner
            .lock()
            .map_err(|_| "状態を確認できません。")?;
        s.demo.prepare(&options);
        let asr_needed = s.demo.raw.is_none();
        s.lab = None;
        s.active = Some(epoch);
        s.status = Status {
            generation: epoch,
            mode: "demo".into(),
            working: true,
            phase: "preparing".into(),
            message: "デモ用モデルを検証しています。".into(),
            total: if options.resident {
                match options.stage.as_str() {
                    "prepare" | "audio" => 1,
                    "release" => 0,
                    _ => 2 * options.repeats,
                }
            } else {
                usize::from(asr_needed)
                    + if options.stage == "audio" {
                        0
                    } else {
                        2 * options.repeats
                    }
            },
            ..Status::default()
        };
        // Previous stages remain visible, but stale results for this stage do not.
        s.demo.rows.retain(|r| r["stage"] != options.stage);
    }
    let worker_app = app.clone();
    let owner = comparisons.inner().clone();
    let recorder = recorder.inner().clone();
    let resident_mode = options.resident;
    let preparation = options.resident && options.stage == "prepare";
    let release = options.resident && options.stage == "release";
    *handle = Some(std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let result = if options.resident {
                if options.stage == "release" {
                    owner.resident.release();
                    Ok(())
                } else {
                    owner.run_resident_demo(&worker_app, &recorder, &options, epoch)
                }
            } else {
                owner.resident.release();
                owner.run_demo(&worker_app, &recorder, options, epoch)
            };
            if result.is_err() {
                owner.resident.release();
            }
            result
        }))
        .unwrap_or_else(|_| Err("デモの実行が中断されました。".into()));
        owner.cleanup_demo_runtime(epoch, result.is_err());
        let success = result.is_ok();
        owner.finish(epoch, result.err());
        if success
            && resident_mode
            && owner.current(epoch)
            && let Ok(mut s) = owner.inner.lock()
        {
            s.status.message = if preparation {
                    "常駐・画像の準備が完了しました。①〜③を実行できます。"
                } else if release {
                    "常駐モデルを解放しました。"
                } else {
                    "常駐デモが完了しました。通常入力へ戻るときは常駐モデルを解放してから音声モデルを再読み込みしてください。"
                }
                .into();
        }
    }));
    Ok(())
}
impl Comparisons {
    pub(super) fn cleanup_demo_runtime(&self, epoch: u64, failed: bool) {
        // run_resident_demo has dropped its server guard. Cancellation may have
        // skipped that locked server even when the last request returned Ok.
        if failed || !self.current(epoch) {
            self.resident.release();
        }
    }
    pub(super) fn demo_message(&self, epoch: u64, message: &str) {
        if let Ok(mut s) = self.inner.lock()
            && self.current(epoch)
            && s.active == Some(epoch)
        {
            s.status.message = message.into();
            s.status.phase = "running".into();
        }
    }
    pub(super) fn demo_row(&self, epoch: u64, row: Value) {
        if let Ok(mut s) = self.inner.lock()
            && self.current(epoch)
            && s.active == Some(epoch)
        {
            s.demo.rows.push(row);
            s.status.completed += 1;
        }
    }
    fn run_demo(
        &self,
        app: &tauri::AppHandle,
        recorder: &crate::recorder::Recorder,
        options: Options,
        epoch: u64,
    ) -> Result<(), String> {
        recorder.suspend_for_comparison()?;
        let root = app
            .path()
            .resource_dir()
            .map_err(|_| "デモのリソースを確認できません。")?;
        let bin = root.join("bin");
        let assets = root.join("lab");
        let models = app.state::<local_models::Manager>();
        let config = protocol::config();
        let raw = self
            .inner
            .lock()
            .map_err(|_| "状態を確認できません。")?
            .demo
            .raw
            .clone();
        let raw = if let Some(raw) = raw {
            raw
        } else {
            let model = models
                .comparison_path("large-v3", || !self.current(epoch))
                .map_err(str::to_owned)?;
            verify_asset(
                &assets.join("speech.wav"),
                config["audio_sha256"].as_str().unwrap(),
                &self.epoch,
                epoch,
            )?;
            self.demo_message(
                epoch,
                &format!(
                    "同じ同梱音声をWhisper large-v3で認識中（ヒントなし・{}）…",
                    options.backend()
                ),
            );
            let audio = local_transcription::read_wav(&assets.join("speech.wav"))
                .map_err(|_| "同梱音声を読み込めません。")?;
            let mut packet = serde_json::to_vec(&json!({"samples":audio.len(),"hint":""})).unwrap();
            packet.push(b'\n');
            for sample in audio {
                packet.extend_from_slice(&sample.to_le_bytes());
            }
            let started = Instant::now();
            let out = crate::correction::child_output(
                Command::new(bin.join("compare-transcription"))
                    .arg(model)
                    .arg(options.backend()),
                packet,
                &self.epoch,
                epoch,
                Duration::from_secs(600),
            )?;
            if !out.status.success() {
                return Err("音声認識に失敗しました。".into());
            }
            let report: Value = serde_json::from_slice(&out.stdout)
                .map_err(|_| "音声認識結果を読み込めません。")?;
            let raw = report["text"]
                .as_str()
                .ok_or("音声認識結果がありません。")?
                .to_owned();
            if raw.len() > 8192 {
                return Err("音声認識の出力が長すぎます。".into());
            }
            self.demo_row(epoch,json!({"stage":"audio","backend":options.backend(),"label":"音声だけ","raw":raw,"final":raw,"elapsedMs":started.elapsed().as_millis() as u64,"status":"completed","asr":report,"audioSha256":config["audio_sha256"]}));
            // Keep the real output visible even when it cannot enter the bounded follow-up.
            protocol::target(&raw)?;
            {
                let mut s = self.inner.lock().map_err(|_| "状態を確認できません。")?;
                if self.current(epoch) {
                    s.demo.raw = Some(raw.clone());
                }
            }
            raw
        };
        if options.stage == "audio" {
            return Ok(());
        }
        let model = models
            .comparison_path("demo-vlm", || !self.current(epoch))
            .map_err(str::to_owned)?;
        let image = options.stage == "image";
        let projector = if image {
            Some(
                models
                    .comparison_path("demo-projector", || !self.current(epoch))
                    .map_err(str::to_owned)?,
            )
        } else {
            None
        };
        for repeat in 0..options.repeats {
            for index in if repeat == 0 { [0, 1] } else { [1, 0] } {
                if !self.current(epoch) {
                    return Err("デモを中止しました。".into());
                }
                let label = if image {
                    ["雨の画像", "屋根からの飴まき画像"][index]
                } else {
                    ["台風の文章", "ハロウィンの文章"][index]
                };
                self.demo_message(
                    epoch,
                    &format!(
                        "{label}をQwen3-VLで判断中（{}回目・{}指定）…",
                        repeat + 1,
                        options.backend()
                    ),
                );
                let prompt = protocol::prompt(
                    &raw,
                    if image {
                        None
                    } else {
                        Some(&options.texts[index])
                    },
                )?;
                let mut command = Command::new(bin.join(if image {
                    "llama-mtmd-cli"
                } else {
                    "llama-completion"
                }));
                command.arg("-m").arg(&model);
                if let Some(projector) = &projector {
                    let name = if index == 0 {
                        "rainfall.png"
                    } else {
                        "candy-shower.png"
                    };
                    verify_asset(
                        &assets.join(name),
                        config["images"][index]["sha256"].as_str().unwrap(),
                        &self.epoch,
                        epoch,
                    )?;
                    command
                        .arg("--mmproj")
                        .arg(projector)
                        .args(["--image-max-tokens", "4096"])
                        .arg("--image")
                        .arg(assets.join(name));
                } else {
                    command.args(["-cnv", "-st", "--simple-io", "--no-display-prompt"]);
                }
                command.args(protocol::backend_args(options.use_gpu, image));
                command
                    .args([
                        "-c",
                        "8192",
                        "-n",
                        "128",
                        "--temp",
                        "0",
                        "--seed",
                        "0",
                        "--threads",
                        "4",
                        "--no-warmup",
                        "--json-schema",
                    ])
                    .arg(protocol::schema(image).to_string())
                    .arg("-p")
                    .arg(&prompt);
                let started = Instant::now();
                let result = crate::correction::child_output(
                    &mut command,
                    Vec::new(),
                    &self.epoch,
                    epoch,
                    Duration::from_secs(1800),
                );
                let mut row = json!({"stage":options.stage,"label":label,"repeat":repeat,"raw":raw,"final":raw,"prompt":prompt,"context":if image {""} else {&options.texts[index]},"status":"error","backend":options.backend(),"ocr":false});
                let parsed = (|| -> Result<Value, String> {
                    let out = result?;
                    let output = String::from_utf8(out.stdout)
                        .map_err(|_| "モデル出力を読み込めません。")?;
                    row["modelOutput"] = output.clone().into();
                    if !out.status.success() {
                        return Err(
                            "Qwen3-VLの実行に失敗しました。メモリ容量を確認してください。".into(),
                        );
                    }
                    protocol::parse(&output, image)
                })();
                row["elapsedMs"] = (started.elapsed().as_millis() as u64).into();
                match parsed {
                    Ok(value) => {
                        let spelling = value["spelling"].as_str().unwrap();
                        row["final"] = protocol::apply(&raw, spelling)?.into();
                        row["spelling"] = spelling.into();
                        row["observation"] = value["observation"].clone();
                        row["status"] = if spelling == "不明" {
                            "undetermined"
                        } else {
                            "completed"
                        }
                        .into();
                        self.demo_row(epoch, row);
                    }
                    Err(error) => {
                        row["error"] = error.clone().into();
                        self.demo_row(epoch, row);
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}
pub(super) fn verify_asset(
    path: &Path,
    expected: &str,
    epoch: &AtomicU64,
    current: u64,
) -> Result<(), String> {
    let out = crate::correction::child_output(
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path),
        Vec::new(),
        epoch,
        current,
        Duration::from_secs(90),
    )?;
    if !out.status.success()
        || std::str::from_utf8(&out.stdout)
            .ok()
            .and_then(|s| s.split_whitespace().next())
            != Some(expected)
    {
        return Err("同梱デモ素材のSHA-256が一致しません。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resident_audio_keeps_preparation_and_release_keeps_evidence() {
        let mut options: Options = serde_json::from_value(json!({
            "stage":"audio", "repeats":1, "texts":["",""], "resident":true
        }))
        .unwrap();
        let mut session = Session {
            resident: true,
            backend: Some("cpu".into()),
            raw: Some("雨がすごいですね".into()),
            rows: vec![json!({"stage":"prepare"}), json!({"stage":"text"})],
        };
        session.prepare(&options);
        assert!(session.raw.is_none());
        assert_eq!(session.rows, vec![json!({"stage":"prepare"})]);
        options.stage = "release".into();
        options.use_gpu = true;
        session.prepare(&options);
        assert_eq!(session.rows.len(), 1);
        assert_eq!(session.backend.as_deref(), Some("cpu"));
    }
    #[test]
    fn backend_switch_discards_previous_asr_and_results() {
        let mut options: Options = serde_json::from_value(json!({
            "stage":"image", "repeats":1, "texts":["", ""]
        }))
        .unwrap();
        assert_eq!(options.backend(), "cpu");
        let mut session = Session {
            resident: false,
            raw: Some("雨がすごいですね".into()),
            backend: Some("cpu".into()),
            rows: vec![json!({"stage":"text"})],
        };
        session.prepare(&options);
        assert!(session.raw.is_some());
        assert_eq!(session.rows.len(), 1);
        options.use_gpu = true;
        session.prepare(&options);
        assert!(session.raw.is_none());
        assert!(session.rows.is_empty());
        assert_eq!(session.backend.as_deref(), Some("metal"));
        session.raw = Some("雨がすごいですね".into());
        session.rows.push(json!({"stage":"audio"}));
        options.use_gpu = false;
        session.prepare(&options);
        assert!(session.raw.is_none());
        assert!(session.rows.is_empty());
    }
}
