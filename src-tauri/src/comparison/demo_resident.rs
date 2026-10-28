//! Explicitly prepared, owned local workers. Preparation never supplies a cached final answer.
use super::*;
use crate::{correction_pool::Pool, demo_protocol as protocol};
use serde_json::{Value, json};
use std::{
    io::Read,
    net::TcpListener,
    process::{Child, Stdio},
};

pub(super) struct Server {
    child: Child,
    url: String,
    token: String,
    backend: String,
    observations: Vec<String>,
    preparation_ms: u64,
    asr_model: PathBuf,
    audio: Vec<f32>,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub(super) struct Runtime {
    pub asr: Pool,
    pub server: Mutex<Option<Server>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            asr: Pool::persistent(),
            server: Mutex::new(None),
        }
    }
}
impl Runtime {
    pub fn release(&self) {
        self.asr.release();
        if let Ok(mut server) = self.server.try_lock() {
            server.take();
        }
    }
    pub fn ready(&self, backend: &str) -> bool {
        self.server.try_lock().is_ok_and(|mut slot| {
            slot.as_mut().is_some_and(|s| {
                s.backend == backend
                    && s.observations.len() == 2
                    && matches!(s.child.try_wait(), Ok(None))
            })
        })
    }
}
fn encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for shift in [18, 12] {
            output.push(TABLE[((n >> shift) & 63) as usize] as char);
        }
        output.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}
impl Server {
    fn request(
        &mut self,
        body: Value,
        epoch: &AtomicU64,
        current: u64,
        timeout: Duration,
    ) -> Result<Value, String> {
        if !matches!(self.child.try_wait(), Ok(None)) {
            return Err("常駐モデルが終了しました。再準備してください。".into());
        }
        let out = crate::correction::child_output(
            Command::new("/usr/bin/curl")
                .args([
                    "--silent",
                    "--show-error",
                    "--fail",
                    "--noproxy",
                    "*",
                    "--max-time",
                ])
                .arg(format!("{:.3}", timeout.as_secs_f64().max(0.001)))
                .args(["-H", "Content-Type: application/json", "-H"])
                .arg(format!("Authorization: Bearer {}", self.token))
                .args(["--data-binary", "@-"])
                .arg(format!("{}/v1/chat/completions", self.url)),
            serde_json::to_vec(&body).unwrap(),
            epoch,
            current,
            timeout,
        )?;
        if !out.status.success() {
            return Err("常駐モデルの応答が期限内に得られませんでした。".into());
        }
        let response: Value =
            serde_json::from_slice(&out.stdout).map_err(|_| "常駐モデルの応答を読めません。")?;
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("常駐モデルの回答がありません。")?;
        serde_json::from_str(content).map_err(|_| "常駐モデルのJSONが不正です。".into())
    }
}
fn body(content: Value, schema: Value) -> Value {
    json!({"messages":[{"role":"user","content":content}],"temperature":0,"seed":0,
        "max_tokens":128,"stream":false,"cache_prompt":false,
        "response_format":{"type":"json_schema","json_schema":{"name":"demo","strict":true,"schema":schema}}})
}
impl Comparisons {
    pub(super) fn run_resident_demo(
        &self,
        app: &tauri::AppHandle,
        recorder: &crate::recorder::Recorder,
        options: &demo::Options,
        epoch: u64,
    ) -> Result<(), String> {
        recorder.suspend_for_comparison()?;
        let root = app
            .path()
            .resource_dir()
            .map_err(|_| "リソースを確認できません。")?;
        let bin = root.join("bin");
        let assets = root.join("lab");
        let models = app.state::<local_models::Manager>();
        let backend = options.backend();
        if options.stage == "prepare" {
            self.resident.release();
            let started = Instant::now();
            let asr_model = models
                .comparison_path("large-v3", || !self.current(epoch))
                .map_err(str::to_owned)?;
            demo::verify_asset(
                &assets.join("speech.wav"),
                protocol::config()["audio_sha256"].as_str().unwrap(),
                &self.epoch,
                epoch,
            )?;
            let audio = local_transcription::read_wav(&assets.join("speech.wav"))
                .map_err(|_| "音声を読めません。")?;
            let model = models
                .comparison_path("demo-vlm", || !self.current(epoch))
                .map_err(str::to_owned)?;
            let projector = models
                .comparison_path("demo-projector", || !self.current(epoch))
                .map_err(str::to_owned)?;
            let listener =
                TcpListener::bind("127.0.0.1:0").map_err(|_| "ローカルポートを準備できません。")?;
            let port = listener
                .local_addr()
                .map_err(|_| "ローカルポートを確認できません。")?
                .port();
            let mut entropy = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut entropy))
                .map_err(|_| "認証キーを生成できません。")?;
            let token = entropy
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            drop(listener);
            let child = Command::new(bin.join("llama-server"))
                .arg("-m")
                .arg(model)
                .arg("--mmproj")
                .arg(projector)
                .args(protocol::backend_args(options.use_gpu, true))
                .args(["--host", "127.0.0.1", "--port"])
                .arg(port.to_string())
                .arg("--api-key")
                .arg(&token)
                .args([
                    "-c",
                    "8192",
                    "--parallel",
                    "1",
                    "--threads",
                    "4",
                    "--image-max-tokens",
                    "4096",
                    "--no-webui",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "常駐モデルを開始できません。")?;
            let mut server = Server {
                child,
                url: format!("http://127.0.0.1:{port}"),
                token,
                backend: backend.into(),
                observations: vec![],
                preparation_ms: 0,
                asr_model,
                audio,
            };
            self.demo_message(epoch, "常駐モデルの読み込み・ウォームアップ中…");
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                if !self.current(epoch) {
                    return Err("準備を中止しました。".into());
                }
                if !matches!(server.child.try_wait(), Ok(None)) || Instant::now() > deadline {
                    return Err("常駐モデルを準備できません。".into());
                }
                let out = crate::correction::child_output(
                    Command::new("/usr/bin/curl")
                        .args([
                            "--silent",
                            "--fail",
                            "--noproxy",
                            "*",
                            "--max-time",
                            "1",
                            "-H",
                        ])
                        .arg(format!("Authorization: Bearer {}", server.token))
                        .arg(format!("{}/health", server.url)),
                    vec![],
                    &self.epoch,
                    epoch,
                    Duration::from_secs(2),
                );
                if out.is_ok_and(|o| o.status.success()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            for (index, name) in ["rainfall.png", "candy-shower.png"].iter().enumerate() {
                self.demo_message(
                    epoch,
                    &format!("画像の観察を事前準備中（{}/2）…", index + 1),
                );
                demo::verify_asset(
                    &assets.join(name),
                    protocol::config()["images"][index]["sha256"]
                        .as_str()
                        .unwrap(),
                    &self.epoch,
                    epoch,
                )?;
                let bytes = std::fs::read(assets.join(name)).map_err(|_| "画像を読めません。")?;
                let content = json!([
                    {"type":"text","text":"画像に実際に見える物や動作を日本語で簡潔に述べてください。見えない状況は補わず、JSONのobservationだけを返してください。"},
                    {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}",encode(&bytes))}}
                ]);
                let value=server.request(body(content,json!({"type":"object","properties":{"observation":{"type":"string","minLength":1,"maxLength":160}},"required":["observation"],"additionalProperties":false})),&self.epoch,epoch,Duration::from_secs(1800))?;
                let observation = value["observation"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.chars().count() <= 160)
                    .ok_or("画像の観察が不正です。")?;
                server.observations.push(observation.into());
            }
            // Warm text inference with neutral content; do not cache a demo answer.
            let warm = protocol::prompt("雨がすごいですね", Some("手がかりはありません。"))?;
            server.request(
                body(json!(warm), protocol::schema(false)),
                &self.epoch,
                epoch,
                Duration::from_secs(1800),
            )?;
            self.resident_asr(&bin, &server.asr_model, &server.audio, epoch, backend)?;
            server.preparation_ms = started.elapsed().as_millis() as u64;
            let observations = server.observations.clone();
            let preparation_ms = server.preparation_ms;
            if !self.current(epoch) {
                self.resident.release();
                return Err("準備を中止しました。".into());
            }
            *self
                .resident
                .server
                .lock()
                .map_err(|_| "状態を確認できません。")? = Some(server);
            self.demo_row(epoch,json!({"stage":"prepare","label":"常駐・画像観察の事前準備","raw":"","final":"準備完了","status":"completed","backend":backend,"elapsedMs":preparation_ms,"preparationMs":preparation_ms,"observations":observations,"timingScope":"preparation"}));
            return Ok(());
        }
        if !self.resident.ready(backend) {
            return Err("選択した方式で「常駐・画像を事前準備」を実行してください。".into());
        }
        self.demo_message(epoch, "読み込み済みのWhisperで音声を新たに認識中…");
        let (asr_model, audio) = {
            let slot = self
                .resident
                .server
                .lock()
                .map_err(|_| "状態を確認できません。")?;
            let server = slot.as_ref().ok_or("再準備してください。")?;
            (server.asr_model.clone(), server.audio.clone())
        };
        let started = Instant::now();
        let report = self.resident_asr(&bin, &asr_model, &audio, epoch, backend)?;
        if report["model_reused"] != true {
            return Err("音声モデルが再読み込みされました。再準備してください。".into());
        }
        let asr_ms = started.elapsed().as_millis() as u64;
        let raw = report["text"]
            .as_str()
            .ok_or("音声認識結果がありません。")?
            .to_owned();
        protocol::target(&raw)?;
        {
            let mut s = self.inner.lock().map_err(|_| "状態を確認できません。")?;
            if self.current(epoch) {
                s.demo.raw = Some(raw.clone());
            }
        }
        if options.stage == "audio" {
            self.demo_row(epoch,json!({"stage":"audio","label":"音声だけ","raw":raw,"final":raw,"status":"completed","backend":backend,"elapsedMs":asr_ms,"asr":report,"resident":true,"timingScope":"prepared-audio-to-result","withinTarget":asr_ms<=3000}));
            return Ok(());
        }
        for repeat in 0..options.repeats {
            for index in if repeat == 0 { [0, 1] } else { [1, 0] } {
                if !self.current(epoch) {
                    return Err("デモを中止しました。".into());
                }
                let image = options.stage == "image";
                let mut slot = self
                    .resident
                    .server
                    .lock()
                    .map_err(|_| "状態を確認できません。")?;
                let server = slot.as_mut().ok_or("常駐モデルを再準備してください。")?;
                let context = if image {
                    server.observations[index].clone()
                } else {
                    options.texts[index].clone()
                };
                let prompt = protocol::prompt(&raw, Some(&context))?;
                let started = Instant::now();
                let result = if asr_ms >= 3000 {
                    Err("音声認識で3秒の目標に達したため原文を保持しました。".into())
                } else {
                    server.request(
                        body(json!(prompt), protocol::schema(false)),
                        &self.epoch,
                        epoch,
                        Duration::from_millis(3000 - asr_ms),
                    )
                };
                let correction_ms = started.elapsed().as_millis() as u64;
                let mut final_text = raw.clone();
                let mut status = "deadline";
                let mut reason = String::new();
                match result {
                    Ok(value) => {
                        let parsed = protocol::parse(&value.to_string(), false)?;
                        final_text = protocol::apply(&raw, parsed["spelling"].as_str().unwrap())?;
                        status = if parsed["spelling"] == "不明" {
                            "undetermined"
                        } else {
                            "completed"
                        };
                    }
                    Err(error) => {
                        reason = error;
                    }
                }
                let elapsed = asr_ms + correction_ms;
                if elapsed > 3000 {
                    final_text = raw.clone();
                    status = "deadline";
                }
                self.demo_row(epoch,json!({"stage":options.stage,"label":if image {["雨の画像","屋根からの飴まき画像"][index]}else{["台風の文章","ハロウィンの文章"][index]},
                    "repeat":repeat,"raw":raw,"final":final_text,"status":status,"error":reason,"backend":backend,"elapsedMs":elapsed,
                    "asrMs":asr_ms,"correctionMs":correction_ms,"asrShared":true,"asr":report,"resident":true,
                    "preparationMs":server.preparation_ms,"observation":if image {Some(context.clone())}else{None},
                    "context":context,"prompt":prompt,"withinTarget":elapsed<=3000,"timingScope":"prepared-audio-to-result"}));
                if status == "deadline" {
                    // A timed-out native generation must not queue behind the next request.
                    slot.take();
                    self.resident.asr.release();
                    return Err(
                        "3秒目標に収まらなかったため原文を保持しました。再準備してください。"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
    fn resident_asr(
        &self,
        bin: &Path,
        model: &Path,
        audio: &[f32],
        epoch: u64,
        backend: &str,
    ) -> Result<Value, String> {
        let bytes = self.resident.asr.request(
            &bin.join("compare-transcription"),
            model.to_str().ok_or("モデルパスが不正です。")?,
            backend,
            serde_json::to_vec(&json!({"audio":audio})).unwrap(),
            &self.epoch,
            epoch,
            Duration::from_secs(600),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| "音声認識結果を読めません。".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn image_encoding_preserves_padding() {
        for (input, expected) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (&[0, 255, 1], "AP8B"),
        ] {
            assert_eq!(super::encode(input), expected);
        }
    }
}
