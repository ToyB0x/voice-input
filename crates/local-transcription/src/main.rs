use local_transcription::{Backend, Transcriber, read_wav};
use serde::Serialize;
use std::{path::Path, time::Instant};

#[derive(Serialize)]
struct Report {
    text: String,
    language: &'static str,
    backend: &'static str,
    audio_seconds: f64,
    load_ms: u128,
    inference_ms: u128,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) || (args.len() == 3 && args[2] != "--cpu") {
        return Err("usage: local-transcription MODEL.bin AUDIO.wav [--cpu]".into());
    }
    let (backend, backend_name) = (Backend::Cpu, "cpu");
    #[cfg(target_os = "macos")]
    let (backend, backend_name) = if args.len() == 2 {
        (Backend::Metal, "metal")
    } else {
        (backend, backend_name)
    };
    let audio = read_wav(Path::new(&args[1]))?;
    let start = Instant::now();
    let engine = Transcriber::load(Path::new(&args[0]), backend)?;
    let load_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let text = engine.transcribe(&audio)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&Report {
            text,
            language: "ja",
            backend: backend_name,
            audio_seconds: audio.len() as f64 / 16000.0,
            load_ms,
            inference_ms: start.elapsed().as_millis(),
        })?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
