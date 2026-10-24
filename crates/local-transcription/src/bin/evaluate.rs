//! Batch benchmark using the production transcriber, with one retained model.
use local_transcription::{Backend, Transcriber, read_wav};
use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path},
    time::Instant,
};

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    path: String,
}
#[derive(Serialize)]
struct Observation {
    text: String,
    inference_ms: f64,
}
#[derive(Serialize)]
struct CaseReport {
    id: String,
    audio_seconds: f64,
    observations: Vec<Observation>,
}
#[derive(Serialize)]
struct Report {
    model_name: String,
    backend: &'static str,
    load_ms: f64,
    cold_first_inference: Observation,
    cases: Vec<CaseReport>,
}
fn infer(engine: &Transcriber, audio: &[f32]) -> Result<Observation, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let text = engine.transcribe(audio)?;
    Ok(Observation {
        text,
        inference_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) || (args.len() == 3 && args[2] != "--cpu") {
        return Err("usage: evaluate MODEL.bin CORPUS.json [--cpu]".into());
    }
    let (backend, backend_name) = (Backend::Cpu, "cpu");
    #[cfg(target_os = "macos")]
    let (backend, backend_name) = if args.len() == 2 {
        (Backend::Metal, "metal")
    } else {
        (backend, backend_name)
    };
    let corpus_path = Path::new(&args[1]);
    let corpus: Corpus = serde_json::from_reader(std::fs::File::open(corpus_path)?)?;
    if corpus.cases.is_empty() || corpus.cases.len() > 32 {
        return Err("corpus must contain 1–32 cases".into());
    }
    let started = Instant::now();
    let engine = Transcriber::load(Path::new(&args[0]), backend)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut cold = None;
    let mut cases = Vec::new();
    for case in corpus.cases {
        eprintln!("evaluate: case {}", case.id);
        if !Path::new(&case.path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err("case paths must stay within the corpus directory".into());
        }
        let audio = read_wav(
            &corpus_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(case.path),
        )?;
        if cold.is_none() {
            cold = Some(infer(&engine, &audio)?);
        }
        let mut observations = Vec::new();
        for _ in 0..2 {
            observations.push(infer(&engine, &audio)?);
        }
        cases.push(CaseReport {
            id: case.id,
            audio_seconds: audio.len() as f64 / 16_000.0,
            observations,
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&Report {
            model_name: engine.model_name(),
            backend: backend_name,
            load_ms,
            cold_first_inference: cold.ok_or("missing first inference")?,
            cases,
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
