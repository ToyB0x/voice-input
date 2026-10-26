//! Explicit controlled experiment. Shares the comparison reservation, cancellation and quit worker.
use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::Stdio,
    sync::mpsc,
};

const TEXTS: [(&str, &str); 2] = [
    (
        "storm",
        "気象情報。大型の台風が接近中。強い雨と風、降水量の増加に注意してください。",
    ),
    (
        "candy",
        "お菓子の新商品。カラフルな飴、キャンディ、砂糖菓子を紹介します。",
    ),
];
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub input: String,
    pub apple: bool,
    pub qwen: bool,
    pub ax: bool,
    pub repeats: usize,
    pub compare_instruction: bool,
    pub instruction: String,
}
impl Options {
    fn validate(&self) -> Result<(), String> {
        if !["bundled", "recorded", "text"].contains(&self.input.as_str())
            || !(1..=2).contains(&self.repeats)
            || (!self.apple && !self.qwen)
            || self.instruction.len() > 1536
            || self.instruction.contains('\0')
        {
            return Err("入力・補正方式・反復回数・追加指示を確認してください。".into());
        }
        Ok(())
    }
}
pub(super) struct Session {
    pub report: Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabStatus {
    pub run_id: Option<String>,
    pub working: bool,
    pub phase: String,
    pub message: String,
    pub planned: usize,
    pub completed: usize,
    pub rows: Vec<Value>,
    pub next: usize,
    pub more: bool,
    pub parts: usize,
    pub foundation: Option<FoundationDiagnostic>,
}
fn now_id(epoch: u64) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("device-{n:x}-{epoch:x}")
}
fn unavailable(reason: &str) -> Value {
    json!({"status":"unavailable","text":"","reason":reason})
}

impl Comparisons {
    fn lab_publish(&self, epoch: u64, row: Value) {
        if let Ok(mut state) = self.inner.lock()
            && state.active == Some(epoch)
            && self.current(epoch)
        {
            if let Some(lab) = &mut state.lab {
                lab.report["rows"].as_array_mut().unwrap().push(row);
            }
            state.status.completed += 1;
        }
    }
    fn lab_update(&self, epoch: u64, key: &str, value: Value) {
        if let Ok(mut state) = self.inner.lock()
            && state.active == Some(epoch)
            && self.current(epoch)
            && let Some(lab) = &mut state.lab
        {
            lab.report[key] = value;
        }
    }
    fn start_lab(
        &self,
        app: tauri::AppHandle,
        recorder: crate::recorder::Recorder,
        options: Options,
    ) -> Result<(), String> {
        options.validate()?;
        recorder.ensure_comparison_idle()?;
        let preferences = crate::desktop::preferences(&app)?;
        if options.input != "text" {
            custom_path(&preferences.model_path)?;
        }
        if options.qwen {
            custom_path(&preferences.correction_model_path)?;
        }
        let recorded = if options.input == "recorded" {
            Some(recorder.comparison_input()?)
        } else {
            None
        };
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
        let run_id = now_id(epoch);
        let instructions = if options.compare_instruction {
            json!({"default":"","custom":options.instruction})
        } else {
            json!({"custom":options.instruction})
        };
        let variants = instructions.as_object().unwrap().len();
        let per_scene = if options.input == "text" { 7 } else { 19 };
        let engines = usize::from(options.apple) + usize::from(options.qwen);
        let total = options.repeats * 2 * per_scene * (1 + variants * engines)
            + (4 + 4) * variants * engines;
        {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| "比較状態を確認できません。")?;
            state.active = Some(epoch);
            state.status = Status {
                generation: epoch,
                mode: "context-lab".into(),
                working: true,
                phase: "preparing".into(),
                total,
                ..Status::default()
            };
            state.lab = Some(Session {
                report: json!({"schemaVersion":1,"kind":"device-context-lab","sampleSetVersion":1,
                "reportId":run_id,"appVersion":env!("CARGO_PKG_VERSION"),"sourceRevision":env!("VOICE_INPUT_SOURCE_REVISION"),
                "platform":"macos","machine":std::env::consts::ARCH,"execution":"local-app","networkDenied":false,
                "completed":false,"inputKind":options.input,"instructions":instructions,
                "presentationEngine":if options.apple {"apple-foundation"} else {"qwen3"},"presentationInstruction":"custom",
                "models":{},"foundation":{},"captures":{},"fixture":{},"rows":[],"kanaControls":[],"preservationControls":[],
                "limits":["Controlled built-in scenes; not other-app screen capture or a direct-image VLM.",
                    "One audio reused across both scenes; repeats are not independent speech samples.",
                    "Expected-word selection and guard acceptance do not guarantee meaning preservation.",
                    "Inference uses local model APIs; app-mode does not install a process network sandbox.",
                    "No audio, image bytes, device name or model paths are included in copied JSON."]}),
            });
        }
        let owner = self.clone();
        *handle = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owner.run_lab(&app, &recorder, &preferences, &options, recorded, epoch)
            }))
            .unwrap_or_else(|_| Err("実機検証ワーカーが終了しました。".into()));
            owner.finish(epoch, result.err());
        }));
        Ok(())
    }
    fn run_lab(
        &self,
        app: &tauri::AppHandle,
        recorder: &crate::recorder::Recorder,
        prefs: &crate::settings::Preferences,
        options: &Options,
        recorded: Option<crate::recorder::ComparisonInput>,
        epoch: u64,
    ) -> Result<(), String> {
        recorder.suspend_for_comparison()?;
        let root = app
            .path()
            .resource_dir()
            .map_err(|_| "同梱リソースを確認できません。")?;
        let resources = root.join("bin");
        let assets = root.join("lab");
        let audio = if options.input == "text" {
            Vec::new()
        } else if let Some(input) = recorded {
            local_transcription::prepare_audio(&input.audio, input.rate)
                .map_err(|_| "録音は1〜60秒で比較してください。")?
        } else {
            local_transcription::read_wav(&assets.join("speech.wav"))
                .map_err(|_| "同梱音声を読み込めません。")?
        };
        let pcm: Vec<u8> = audio.iter().flat_map(|x| x.to_le_bytes()).collect();
        let audio_hash = if audio.is_empty() {
            Value::Null
        } else {
            json!(hash_stdin(&pcm, &self.epoch, epoch)?)
        };
        let asset_hashes = json!({"storm.png":hash_file(&assets.join("storm.png"),&self.epoch,epoch)?,"candy.png":hash_file(&assets.join("candy.png"),&self.epoch,epoch)?});
        let audio_ms = audio.len() as u64 * 1000 / 16000;
        self.lab_update(epoch,"fixture",json!({"utterance":if options.input=="recorded" {"利用者の録音（期待語は実験シーン側の定義）"} else {"あめがすごいですね"},
            "voice":if options.input=="bundled" {"CIの同梱合成音声（Eddy ja_JP）"} else if options.input=="text" {"音声なし"} else {"利用者の録音"},
            "origin":options.input,"audioMs":audio_ms,"audioSha256":audio_hash,"audioHashFormat":"f32le-16000-mono","hashes":{},"assetHashes":asset_hashes}));
        let mut binaries = serde_json::Map::new();
        for name in [
            "local-correction",
            "compare-transcription",
            "apple-foundation-correction",
            "context-lab-native",
        ] {
            binaries.insert(
                name.into(),
                json!(hash_file(&resources.join(name), &self.epoch, epoch)?),
            );
        }
        self.lab_update(epoch, "binaries", Value::Object(binaries));
        let mut paths = BTreeMap::new();
        let speech_hash = if options.input != "text" {
            paths.insert("current-speech".into(), custom_path(&prefs.model_path));
            json!(hash_file(Path::new(&prefs.model_path), &self.epoch, epoch)?)
        } else {
            Value::Null
        };
        let qwen_hash = if options.qwen {
            paths.insert(
                "current-correction".into(),
                custom_path(&prefs.correction_model_path),
            );
            json!(hash_file(
                Path::new(&prefs.correction_model_path),
                &self.epoch,
                epoch
            )?)
        } else {
            Value::Null
        };
        self.lab_update(epoch,"models",json!({"speech":if options.input=="text" {"なし（かな原文）"} else {"設定中のWhisper"},
            "speechSha256":speech_hash,"speechBackend":if options.input=="text" {"none"} else {prefs.backend.as_str()},"correction":if options.apple {"Apple Foundation Models"} else {"設定中のQwen"},
            "qwenSha256":qwen_hash,"qwenBackend":prefs.correction_backend,"qwenModelReuse":false,"afmSessionReuse":false}));
        let apple = self.foundation_check(&resources, epoch);
        if let Ok(state) = self.inner.lock()
            && let Some(d) = &state.status.foundation
        {
            let value = d
                .status
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok())
                .unwrap_or_else(|| json!({"availability":d.error}));
            drop(state);
            self.lab_update(epoch, "foundation", value);
        }
        let mut captures = BTreeMap::new();
        for (scene, text) in TEXTS {
            if !self.current(epoch) {
                return Ok(());
            }
            self.message(
                epoch,
                "capturing",
                &format!("{scene}: 同梱ウィンドウのAX・OCR・画像分類を取得しています。"),
            );
            let capture = capture_scene(
                &resources.join("context-lab-native"),
                &assets,
                scene,
                text,
                options.ax,
                &self.epoch,
                epoch,
            )?;
            captures.insert(scene.to_owned(), capture);
            self.lab_update(epoch, "captures", json!(captures));
        }
        let instructions = if options.compare_instruction {
            vec![("default", ""), ("custom", options.instruction.as_str())]
        } else {
            vec![("custom", options.instruction.as_str())]
        };
        let engines: Vec<_> = [("apple-foundation", options.apple), ("qwen3", options.qwen)]
            .into_iter()
            .filter_map(|(x, on)| on.then_some(x))
            .collect();
        for repeat in 0..options.repeats {
            let mut cache: BTreeMap<String, Row> = BTreeMap::new();
            for scene in if repeat % 2 == 0 {
                ["storm", "candy"]
            } else {
                ["candy", "storm"]
            } {
                let capture = &captures[scene];
                let other = &captures[if scene == "storm" { "candy" } else { "storm" }];
                let sources = scene_sources(capture, other);
                let mut conditions = vec![("none", "none", Some(String::new()))];
                for (source, context) in &sources {
                    for route in if options.input == "text" {
                        &["correction"][..]
                    } else {
                        &["hint", "correction", "both"][..]
                    } {
                        conditions.push((*source, *route, context.clone()));
                    }
                }
                if repeat % 2 == 1 {
                    conditions.reverse();
                }
                for (source, route, context) in conditions {
                    if !self.current(epoch) {
                        return Ok(());
                    }
                    self.message(
                        epoch,
                        "running",
                        &format!("{scene} / {source} / {route} を直列比較中。"),
                    );
                    let expected = if scene == "storm" { "雨" } else { "飴" };
                    let opposite = if scene == "storm" { "飴" } else { "雨" };
                    let base = json!({"scene":scene,"source":source,"route":route,"repeat":repeat,"expected":expected,"opposite":opposite,"audioSha256":audio_hash});
                    let variants = std::iter::once(("none", "none", "")).chain(
                        engines.iter().flat_map(|engine| {
                            instructions
                                .iter()
                                .map(move |(key, value)| (*engine, *key, *value))
                        }),
                    );
                    let Some(context) = context else {
                        for (engine, instruction, _) in variants {
                            let mut r = base.clone();
                            r["engine"] = engine.into();
                            r["instruction"] = instruction.into();
                            r["status"] = "unavailable".into();
                            r["reason"] = "context-unavailable".into();
                            self.lab_publish(epoch, r);
                        }
                        continue;
                    };
                    let hint = local_transcription::recognition_hint(
                        if ["hint", "both"].contains(&route) {
                            &context
                        } else {
                            ""
                        },
                    );
                    let shared = cache.contains_key(&hint);
                    let asr = if options.input == "text" {
                        let mut r = Row::new(0, lab_case("none", prefs, route));
                        r.raw = Some("あめがすごいですね".into());
                        r.final_text = r.raw.clone();
                        r.status = "uncorrected".into();
                        r
                    } else {
                        cache
                            .entry(hint.clone())
                            .or_insert_with(|| {
                                run_case(
                                    &resources,
                                    &lab_case("none", prefs, route),
                                    0,
                                    &audio,
                                    &crate::context::Sources {
                                        ax: context.clone(),
                                        ocr: String::new(),
                                    },
                                    &paths,
                                    &apple,
                                    None,
                                    "",
                                    &self.epoch,
                                    epoch,
                                )
                            })
                            .clone()
                    };
                    for (engine, key, instruction) in variants {
                        if !self.current(epoch) {
                            return Ok(());
                        }
                        let mut row = asr.clone();
                        if engine != "none"
                            && let Some(raw) = &asr.raw
                        {
                            let case = lab_case(engine, prefs, route);
                            if let Err(error) = correct_text(
                                &resources,
                                &case,
                                &mut row,
                                raw,
                                &context,
                                instruction,
                                &paths,
                                &apple,
                                &self.epoch,
                                epoch,
                            ) {
                                record_error(&mut row, error);
                            }
                        }
                        let mut r = base.clone();
                        r["engine"] = engine.into();
                        r["instruction"] = key.into();
                        r["context"] = context.clone().into();
                        r["status"] = row.status.clone().into();
                        r["reason"] = row.message.clone().into();
                        r["raw"] = json!(asr.raw);
                        r["candidate"] = json!(row.candidate);
                        r["finalText"] = json!(row.final_text);
                        r["accepted"] = json!(row.accepted);
                        r["asrMs"] = json!(asr.asr_ms);
                        r["asrLoadMs"] = json!(asr.asr_load_ms);
                        r["asrShared"] = json!(shared);
                        r["correctionMs"] = json!(row.correction_ms);
                        r["judgment"] =
                            judgment(row.final_text.as_deref(), expected, opposite).into();
                        self.lab_publish(epoch, r);
                    }
                }
            }
        }
        let mut kana = Vec::new();
        let mut controls = Vec::new();
        for engine in engines {
            for (key, instruction) in &instructions {
                for (scene, text) in TEXTS {
                    let row = control(
                        &resources,
                        engine,
                        prefs,
                        "あめがすごいですね",
                        text,
                        instruction,
                        &paths,
                        &apple,
                        &self.epoch,
                        epoch,
                    );
                    kana.push(json!({"scene":scene,"engine":engine,"instruction":key,"input":"あめがすごいですね","result":control_result(&row)}));
                    self.lab_update(epoch, "kanaControls", json!(kana));
                    self.lab_publish_control(epoch);
                }
                // Four preservation probes plus filler and formatting probes, matching the ordinary selftest.
                for raw in [
                    "明日の午後3時に会議室Bで10人の会議をします",
                    "この変更は本番には適用しないでください",
                    "金曜日ではなく木曜日にお願いします",
                    "田中さんへ資料を送ります",
                    "えーまず資料を確認して次に田中さんへ連絡します",
                    "持ち物はパソコンと充電器と社員証です",
                ] {
                    if !self.current(epoch) {
                        return Ok(());
                    }
                    let row = control(
                        &resources,
                        engine,
                        prefs,
                        raw,
                        "",
                        instruction,
                        &paths,
                        &apple,
                        &self.epoch,
                        epoch,
                    );
                    controls.push(json!({"engine":engine,"input":raw,"instruction":key,"result":control_result(&row),"finalText":row.final_text}));
                    self.lab_update(epoch, "preservationControls", json!(controls));
                    self.lab_publish_control(epoch);
                }
            }
        }
        self.lab_update(epoch, "kanaControls", json!(kana));
        self.lab_update(epoch, "preservationControls", json!(controls));
        Ok(())
    }
    fn lab_publish_control(&self, epoch: u64) {
        if let Ok(mut s) = self.inner.lock()
            && s.active == Some(epoch)
            && self.current(epoch)
        {
            s.status.completed += 1;
        }
    }
}
fn lab_case(engine: &str, prefs: &crate::settings::Preferences, route: &str) -> Case {
    Case {
        speech: "current-speech".into(),
        speech_backend: prefs.backend.clone(),
        correction: if engine == "qwen3" {
            "current-correction".into()
        } else {
            engine.into()
        },
        correction_backend: if engine == "apple-foundation" {
            "system".into()
        } else {
            prefs.correction_backend.clone()
        },
        source: if route == "none" {
            "none".into()
        } else {
            "ax".into()
        },
        route: match route {
            "hint" => "asr-only",
            "correction" => "correction-only",
            "both" => "both",
            _ => "none",
        }
        .into(),
    }
}
#[allow(clippy::too_many_arguments)]
fn control(
    resources: &Path,
    engine: &str,
    prefs: &crate::settings::Preferences,
    raw: &str,
    context: &str,
    instruction: &str,
    paths: &BTreeMap<String, Result<PathBuf, String>>,
    apple: &Result<(), String>,
    epoch: &AtomicU64,
    current: u64,
) -> Row {
    let mut r = Row::new(0, lab_case(engine, prefs, "correction"));
    r.raw = Some(raw.into());
    r.final_text = r.raw.clone();
    if let Err(e) = correct_text(
        resources,
        &r.case.clone(),
        &mut r,
        raw,
        context,
        instruction,
        paths,
        apple,
        epoch,
        current,
    ) {
        record_error(&mut r, e);
    }
    r
}
fn control_result(r: &Row) -> Value {
    json!({"candidate":r.candidate,"accepted":r.accepted,"reason":r.message,"status":r.status,"elapsed_ms":r.correction_ms})
}
fn scene_sources(c: &Value, other: &Value) -> Vec<(&'static str, Option<String>)> {
    fn text(v: &Value) -> Option<String> {
        if v["status"] == "completed" {
            v["text"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
        } else {
            None
        }
    }
    let labels = if c["vision"]["status"] == "completed" {
        c["vision"]["labels"]
            .as_array()
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(format!(
                            "{} ({:.3})",
                            x["label"].as_str()?,
                            x["confidence"].as_f64()?
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
    } else {
        None
    };
    vec![
        ("fixture-text", c["fixtureText"].as_str().map(str::to_owned)),
        ("ax", text(&c["ax"])),
        ("ocr", text(&c["ocr"])),
        ("image-ocr", text(&c["imageOCR"])),
        ("vision-labels", labels),
        (
            "mismatched-text",
            other["fixtureText"].as_str().map(str::to_owned),
        ),
    ]
}
fn judgment(text: Option<&str>, expected: &str, opposite: &str) -> &'static str {
    let Some(text) = text else {
        return "undetermined";
    };
    match (text.contains(expected), text.contains(opposite)) {
        (true, false) => "target",
        (false, true) => "opposite",
        (true, true) => "mixed",
        _ => "undetermined",
    }
}
fn hash_file(path: &Path, epoch: &AtomicU64, current: u64) -> Result<String, String> {
    hash_command(
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path),
        Vec::new(),
        epoch,
        current,
    )
}
fn hash_stdin(bytes: &[u8], epoch: &AtomicU64, current: u64) -> Result<String, String> {
    hash_command(
        Command::new("/usr/bin/shasum").args(["-a", "256"]),
        bytes.to_vec(),
        epoch,
        current,
    )
}
fn hash_command(
    command: &mut Command,
    input: Vec<u8>,
    epoch: &AtomicU64,
    current: u64,
) -> Result<String, String> {
    let out =
        crate::correction::child_output(command, input, epoch, current, Duration::from_secs(90))?;
    let hash = std::str::from_utf8(&out.stdout)
        .ok()
        .and_then(|s| s.split_whitespace().next())
        .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()));
    if !out.status.success() {
        return Err("モデル・音声のハッシュを確認できません。".into());
    }
    hash.map(str::to_owned)
        .ok_or_else(|| "ハッシュの形式を確認できません。".into())
}

fn capture_scene(
    runtime: &Path,
    assets: &Path,
    scene: &str,
    expected: &str,
    ax: bool,
    epoch: &AtomicU64,
    current: u64,
) -> Result<Value, String> {
    // A two-line protocol keeps the controlled window's AppKit loop free while
    // the parent (a different process) performs AX. Only this owned child is read.
    let mut child = Command::new(runtime)
        .arg(assets)
        .arg(scene)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "文脈取得ヘルパーを起動できません。")?;
    let pid = child.id();
    let mut input = child
        .stdin
        .take()
        .ok_or("取得ヘルパーの入力を確認できません。")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("取得ヘルパーの出力を確認できません。")?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        for _ in 0..2 {
            let mut bytes = Vec::new();
            let result = reader.by_ref().take(32769).read_until(b'\n', &mut bytes);
            if result.is_err()
                || bytes.is_empty()
                || bytes.len() > 32768
                || bytes.last() != Some(&b'\n')
            {
                let _ = tx.send(Err("capture-protocol-failed"));
                return;
            }
            if tx.send(Ok(bytes)).is_err() {
                return;
            }
        }
    });
    let started = Instant::now();
    let result = (|| -> Result<Value, String> {
        let receive = || -> Result<Value, String> {
            loop {
                if epoch.load(Ordering::Acquire) != current
                    || started.elapsed() > Duration::from_secs(90)
                {
                    return Err("文脈取得を中止しました。".into());
                }
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(Ok(bytes)) => {
                        return serde_json::from_slice(&bytes)
                            .map_err(|_| "取得結果の形式を確認できません。".into());
                    }
                    Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("取得ヘルパーが終了しました。".into());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        };
        let ready = receive()?;
        if ready["phase"] != "ready" || ready["protocolVersion"] != 1 {
            return Err("取得ヘルパーの準備を確認できません。".into());
        }
        let ax_result = if ax {
            read_fixture_ax(pid, expected)
        } else {
            unavailable("disabled")
        };
        if epoch.load(Ordering::Acquire) != current {
            return Err("文脈取得を中止しました。".into());
        }
        input
            .write_all(b"continue\n")
            .map_err(|_| "文脈取得を続行できません。")?;
        drop(input);
        let mut captured = receive()?;
        if captured["scene"] != scene || captured["fixtureText"] != expected {
            return Err("取得サンプルが一致しません。".into());
        }
        // Never accept uncontrolled input text or paths from a mismatched helper.
        for key in ["ocr", "imageOCR"] {
            if captured[key]["text"]
                .as_str()
                .is_some_and(|s| s.len() > 6144 || s.contains('\0'))
            {
                return Err("取得結果が上限を超えました。".into());
            }
        }
        captured["ax"] = ax_result;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "取得ヘルパーの終了を確認できません。")?
            {
                if !status.success() {
                    return Err("取得ヘルパーが正常終了しませんでした。".into());
                }
                break;
            }
            if epoch.load(Ordering::Acquire) != current
                || started.elapsed() > Duration::from_secs(90)
            {
                return Err("文脈取得を中止しました。".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(captured)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = reader.join();
    result
}
fn read_fixture_ax(pid: u32, expected: &str) -> Value {
    use core_foundation::{
        base::{CFType, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };
    use std::ffi::c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            parameter: CFTypeRef,
            result: *mut CFTypeRef,
        ) -> i32;
        fn AXValueCreate(kind: i32, value: *const c_void) -> CFTypeRef;
    }
    if !crate::insertion::trusted() {
        return unavailable("accessibility-not-trusted");
    }
    let start = Instant::now();
    let result = (|| -> Option<String> {
        unsafe {
            let app = AXUIElementCreateApplication(pid as i32);
            if app.is_null() {
                return None;
            }
            let app = CFType::wrap_under_create_rule(app);
            AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 2.0);
            let field = crate::insertion::attribute(&app, "AXFocusedUIElement").ok()?;
            // Only the known fixture field; never parent/window traversal or AXValue fallback.
            let label = crate::insertion::attribute(&field, "AXDescription")
                .ok()?
                .downcast::<CFString>()?
                .to_string();
            if label != "Voice Input controlled context" {
                return None;
            }
            let range = crate::insertion::Range {
                location: 0,
                length: expected.encode_utf16().count() as isize,
            };
            let parameter = AXValueCreate(4, (&range as *const crate::insertion::Range).cast());
            if parameter.is_null() {
                return None;
            }
            let parameter = CFType::wrap_under_create_rule(parameter);
            let mut result = std::ptr::null();
            let code = AXUIElementCopyParameterizedAttributeValue(
                field.as_CFTypeRef(),
                CFString::new("AXStringForRange").as_concrete_TypeRef(),
                parameter.as_CFTypeRef(),
                &mut result,
            );
            if code != 0 || result.is_null() {
                return None;
            }
            let result = CFType::wrap_under_create_rule(result).downcast::<CFString>()?;
            if result.char_len() > 512 {
                return None;
            }
            let text = result.to_string();
            if text != expected {
                return None;
            }
            Some(text)
        }
    })();
    match result {
        Some(text) => {
            json!({"status":"completed","text":text,"target":"owned controlled fixture child","elapsedMs":start.elapsed().as_millis() as u64})
        }
        None => unavailable("fixture-ax-unavailable"),
    }
}

#[tauri::command]
pub fn start_context_lab(
    app: tauri::AppHandle,
    comparisons: tauri::State<'_, Comparisons>,
    recorder: tauri::State<'_, crate::recorder::Recorder>,
    options: Options,
) -> Result<(), String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    ensure_comparison_available(&app)?;
    comparisons.start_lab(app.clone(), recorder.inner().clone(), options)
}
#[tauri::command]
pub fn context_lab_status(
    comparisons: tauri::State<'_, Comparisons>,
    run_id: Option<String>,
    after: usize,
) -> LabStatus {
    let state = comparisons.inner.lock().unwrap_or_else(|e| e.into_inner());
    let report = state.lab.as_ref().map(|s| &s.report);
    let id = report
        .and_then(|r| r["reportId"].as_str())
        .map(str::to_owned);
    let rows = report.and_then(|r| r["rows"].as_array());
    let total = rows.map_or(0, Vec::len);
    let start = if run_id == id { after.min(total) } else { 0 };
    let end = (start + 12).min(total);
    LabStatus {
        run_id: id,
        working: state.status.working,
        phase: state.status.phase.clone(),
        message: state.status.message.clone(),
        planned: state.status.total,
        completed: state.status.completed,
        rows: rows.map_or_else(Vec::new, |r| r[start..end].to_vec()),
        next: end,
        more: end < total,
        parts: total.div_ceil(20),
        foundation: state.status.foundation.clone(),
    }
}
fn export(
    report: &Value,
    include_text: bool,
    part: Option<usize>,
    phase: &str,
) -> Result<String, String> {
    let mut result = report.clone();
    let rows = result["rows"]
        .as_array()
        .ok_or("検証結果の形式を確認できません。")?;
    let total = rows.len();
    let parts = total.div_ceil(20);
    if part.is_some_and(|p| p >= parts) {
        return Err("分割番号を確認してください。".into());
    }
    let offset = part.unwrap_or(0) * 20;
    if part.is_some() {
        result["rows"] = json!(&rows[offset..(offset + 20).min(total)]);
    }
    result["phase"] = phase.into();
    result["totalRows"] = total.into();
    result["rowOffset"] = if part.is_some() {
        offset.into()
    } else {
        0.into()
    };
    result["partIndex"] = json!(part);
    result["partCount"] = parts.into();
    result["includesText"] = include_text.into();
    if !include_text {
        result["instructions"] = json!({"redacted":true});
        for key in ["kanaControls", "preservationControls"] {
            result[key] = json!([]);
        }
        if let Some(captures) = result["captures"].as_object_mut() {
            for c in captures.values_mut() {
                c.as_object_mut().map(|o| o.remove("fixtureText"));
                for key in ["ax", "ocr", "imageOCR"] {
                    c[key].as_object_mut().map(|o| o.remove("text"));
                }
            }
        }
        if let Some(rows) = result["rows"].as_array_mut() {
            for row in rows {
                if let Some(o) = row.as_object_mut() {
                    for key in ["raw", "candidate", "finalText", "context"] {
                        o.remove(key);
                    }
                }
            }
        }
    }
    let json = serde_json::to_string_pretty(&result).map_err(|_| "JSONを生成できません。")?;
    if json.len() > 1_500_000 {
        return Err("JSONが長いため、20行ずつの分割コピーを使用してください。".into());
    }
    Ok(json)
}
#[tauri::command]
pub fn context_lab_report(
    comparisons: tauri::State<'_, Comparisons>,
    run_id: String,
    include_text: bool,
    part: Option<usize>,
) -> Result<String, String> {
    let state = comparisons
        .inner
        .lock()
        .map_err(|_| "検証結果を確認できません。")?;
    if state.status.working {
        return Err("実行終了後にコピーしてください。".into());
    }
    let report = &state.lab.as_ref().ok_or("検証結果がありません。")?.report;
    if report["reportId"] != run_id {
        return Err("検証結果が変わりました。再確認してください。".into());
    }
    let mut report = report.clone();
    report["runMessage"] = state.status.message.clone().into();
    export(&report, include_text, part, &state.status.phase)
}
#[tauri::command]
pub fn copy_context_lab_report(
    comparisons: tauri::State<'_, Comparisons>,
    run_id: String,
    include_text: bool,
    part: Option<usize>,
) -> Result<(), String> {
    let report = context_lab_report(comparisons, run_id, include_text, part)?;
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(report))
        .map_err(|_| "JSONをコピーできません。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_free_asr_uses_the_none_source_and_retained_labs_block_updates() {
        let prefs = crate::settings::Preferences::default();
        let case = lab_case("none", &prefs, "none");
        assert_eq!(case.source, "none");
        assert_eq!(
            crate::context::Sources {
                ax: String::new(),
                ocr: String::new()
            }
            .for_source(&case.source),
            Some(String::new())
        );
        let owner = Comparisons::new();
        assert!(!owner.has_results());
        owner.inner.lock().unwrap().lab = Some(Session {
            report: json!({"rows":[]}),
        });
        assert!(owner.has_results());
        owner.cancel(true);
        assert!(!owner.has_results());
    }
    #[test]
    #[ignore = "requires the bundled native helper; invoked explicitly by macOS CI"]
    fn controlled_context_helper_real_vision() {
        let runtime =
            PathBuf::from(std::env::var("VOICE_INPUT_LAB_BIN").expect("native helper path"));
        let assets =
            PathBuf::from(std::env::var("VOICE_INPUT_LAB_ASSETS").expect("controlled assets path"));
        let output = PathBuf::from(
            std::env::var("VOICE_INPUT_LAB_EVIDENCE").expect("public CI evidence path"),
        );
        std::fs::create_dir_all(&output).unwrap();
        for (scene, expected) in TEXTS {
            let captured = capture_scene(
                &runtime,
                &assets,
                scene,
                expected,
                true,
                &AtomicU64::new(1),
                1,
            )
            .expect("native controlled capture");
            for key in ["ax", "ocr", "imageOCR", "vision"] {
                assert!(
                    ["completed", "unavailable"]
                        .contains(&captured[key]["status"].as_str().unwrap_or("invalid"))
                );
            }
            std::fs::write(
                output.join(format!("{scene}.json")),
                serde_json::to_vec_pretty(&captured).unwrap(),
            )
            .unwrap();
            eprintln!(
                "{scene}: AX={}, OCR={}, image OCR={}, Vision={}",
                captured["ax"]["status"],
                captured["ocr"]["status"],
                captured["imageOCR"]["status"],
                captured["vision"]["status"]
            );
        }
    }
    #[test]
    fn capture_cancellation_reaps_the_owned_helper() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("voice-lab-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("capture.py");
        std::fs::write(
            &script,
            r#"import json,os,pathlib,sys,time
pathlib.Path(sys.argv[0]+'.pid').write_text(str(os.getpid()))
print(json.dumps(dict(phase='ready',protocolVersion=1)),flush=True)
assert sys.stdin.readline()=='continue\n'
time.sleep(30)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o600)).unwrap();
        let epoch = Arc::new(AtomicU64::new(1));
        let worker_epoch = epoch.clone();
        let path = script.clone();
        let worker = std::thread::spawn(move || {
            capture_scene(
                Path::new("/usr/bin/python3"),
                &path,
                "storm",
                "public",
                false,
                &worker_epoch,
                1,
            )
        });
        let marker = root.join("capture.py.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read_to_string(&marker)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .is_none()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists());
        epoch.fetch_add(1, Ordering::AcqRel);
        assert!(worker.join().unwrap().is_err());
        let pid = std::fs::read_to_string(&marker).unwrap();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn metadata_export_redacts_bodies_instructions_and_control_candidates() {
        let r = json!({"reportId":"run","rows":[{"raw":"PRIVATE","candidate":"PRIVATE","finalText":"PRIVATE","context":"PRIVATE","status":"accepted"}],
            "captures":{"storm":{"fixtureText":"PRIVATE","ax":{"status":"completed","text":"PRIVATE"},"ocr":{"text":"PRIVATE"},"imageOCR":{"text":"PRIVATE"}}},
            "instructions":{"custom":"PRIVATE"},"kanaControls":["PRIVATE"],"preservationControls":["PRIVATE"]});
        assert!(!export(&r, false, None, "done").unwrap().contains("PRIVATE"));
        assert!(export(&r, true, None, "done").unwrap().contains("PRIVATE"));
    }
    #[test]
    fn paged_exports_keep_identity_and_reject_missing_parts() {
        let r = json!({"reportId":"same-run","rows":(0..43).map(|i|json!({"index":i})).collect::<Vec<_>>()});
        for (part, offset, len) in [(0, 0, 20), (1, 20, 20), (2, 40, 3)] {
            let value: Value =
                serde_json::from_str(&export(&r, true, Some(part), "done").unwrap()).unwrap();
            assert_eq!(value["rowOffset"], offset);
            assert_eq!(value["rows"].as_array().unwrap().len(), len);
            assert_eq!(value["reportId"], "same-run");
        }
        assert!(export(&r, true, Some(3), "done").is_err());
    }
    #[test]
    fn context_unavailability_and_word_score_are_explicit() {
        let c = json!({"fixtureText":"fixture","ax":{"status":"unavailable","text":"wrong"},"ocr":{"status":"completed","text":""},"imageOCR":{"status":"unavailable"},"vision":{"status":"completed","labels":[{"label":"actual","confidence":0.8}]}});
        let s = scene_sources(&c, &c);
        assert!(s[1].1.is_none());
        assert!(s[2].1.is_none());
        assert_eq!(s[4].1.as_deref(), Some("actual (0.800)"));
        assert_eq!(judgment(Some("あめ"), "雨", "飴"), "undetermined");
        assert_eq!(judgment(Some("雨と飴"), "雨", "飴"), "mixed");
    }
}
