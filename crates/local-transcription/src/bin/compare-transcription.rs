//! One bounded PCM request over stdin. No audio files, prompt logging or retained model.
use local_transcription::{Backend, MAX_SAMPLES, Transcriber};
use serde::Deserialize;
use std::{
    io::{BufRead, Read, Write},
    path::Path,
    time::Instant,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    samples: usize,
    hint: String,
}
fn run() -> Result<(), &'static str> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 3 && args[2] == "--serve" {
        return serve(&args[..2]);
    }
    if args.len() != 2
        || !Path::new(&args[0]).is_absolute()
        || !["cpu", "metal"].contains(&args[1].as_str())
    {
        return Err("invalid-arguments");
    }
    let mut input = std::io::stdin().lock();
    let mut header = Vec::new();
    input
        .by_ref()
        .take(4097)
        .read_until(b'\n', &mut header)
        .map_err(|_| "invalid-header")?;
    if header.len() > 4096 || header.last() != Some(&b'\n') {
        return Err("invalid-header");
    }
    let header: Header = serde_json::from_slice(&header).map_err(|_| "invalid-header")?;
    if !(16000..=MAX_SAMPLES).contains(&header.samples)
        || header.hint.len() > 768
        || header.hint.contains('\0')
    {
        return Err("input-limit");
    }
    let mut bytes = Vec::new();
    input
        .take((header.samples * 4 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid-pcm")?;
    if bytes.len() != header.samples * 4 {
        return Err("invalid-pcm");
    }
    let audio: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    if audio.iter().any(|s| !s.is_finite() || s.abs() > 1.0) {
        return Err("invalid-pcm");
    }
    let backend = match args[1].as_str() {
        "cpu" => Backend::Cpu,
        #[cfg(target_os = "macos")]
        "metal" => Backend::Metal,
        _ => return Err("backend-unavailable"),
    };
    let started = Instant::now();
    let engine =
        Transcriber::load(Path::new(&args[0]), backend).map_err(|_| "model-load-failed")?;
    let load_ms = started.elapsed().as_millis() as u64;
    let inference = Instant::now();
    let text = engine
        .transcribe_with_hint(&audio, Some(&header.hint))
        .map_err(|_| "inference-failed")?;
    let silent =
        audio.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / (audio.len() as f64) < 1e-10;
    if args[1] == "metal" && !silent && !engine.metal_active() {
        return Err("metal-unavailable");
    }
    if text.len() > 8192 || text.contains('\0') {
        return Err("output-limit");
    }
    serde_json::to_writer(std::io::stdout().lock(),&serde_json::json!({"text":text,"backend":args[1],"silent":silent,"metal_active":engine.metal_active(),"load_ms":load_ms,
        "inference_ms":inference.elapsed().as_millis() as u64,"elapsed_ms":started.elapsed().as_millis() as u64})).map_err(|_|"output-failed")?;
    Ok(())
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(run);
    if !matches!(result, Ok(Ok(()))) {
        let reason = match result {
            Ok(Err(code)) => code,
            _ => "runtime-failed",
        };
        let _ = serde_json::to_writer(
            std::io::stdout().lock(),
            &serde_json::json!({"error":reason}),
        );
        let _ = std::io::stdout().write_all(b"\n");
        std::process::exit(1);
    }
}

// Resident demo protocol: bounded JSON lines, one freshly recognized PCM request at a time.
fn serve(args: &[String]) -> Result<(), &'static str> {
    if !Path::new(&args[0]).is_absolute() {
        return Err("invalid-arguments");
    }
    let backend = match args[1].as_str() {
        "cpu" => Backend::Cpu,
        #[cfg(target_os = "macos")]
        "metal" => Backend::Metal,
        _ => return Err("backend-unavailable"),
    };
    let started = Instant::now();
    let engine =
        Transcriber::load(Path::new(&args[0]), backend).map_err(|_| "model-load-failed")?;
    let load_ms = started.elapsed().as_millis() as u64;
    let mut input = std::io::stdin().lock();
    let mut count = 0;
    loop {
        let mut line = Vec::new();
        input
            .by_ref()
            .take(8_000_001)
            .read_until(b'\n', &mut line)
            .map_err(|_| "invalid-header")?;
        if line.is_empty() {
            return Ok(());
        }
        if line.len() > 8_000_000 || line.last() != Some(&b'\n') {
            return Err("input-limit");
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            audio: Vec<f32>,
        }
        let request: Request = serde_json::from_slice(&line).map_err(|_| "invalid-pcm")?;
        if !(16000..=MAX_SAMPLES).contains(&request.audio.len())
            || request
                .audio
                .iter()
                .any(|s| !s.is_finite() || s.abs() > 1.0)
        {
            return Err("invalid-pcm");
        }
        let started = Instant::now();
        let text = engine
            .transcribe_with_hint(&request.audio, Some(""))
            .map_err(|_| "inference-failed")?;
        if text.len() > 8192 || text.contains('\0') {
            return Err("output-limit");
        }
        if args[1] == "metal" && !engine.metal_active() {
            return Err("metal-unavailable");
        }
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "text":text, "model_reused":count > 0, "load_ms":if count == 0 {load_ms} else {0},
                "inference_ms":started.elapsed().as_millis() as u64, "backend":args[1],
                "metal_active":engine.metal_active()
            }),
        )
        .map_err(|_| "output-failed")?;
        stdout
            .write_all(b"\n")
            .and_then(|_| stdout.flush())
            .map_err(|_| "output-failed")?;
        count += 1;
    }
}
