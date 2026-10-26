//! Public/synthetic CI fixtures only; never bundled with the app.
use local_correction::{Answer, Request, parse_answer};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
#[derive(Deserialize)]
struct Case {
    id: String,
    original: String,
    candidate: String,
}
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Serialize)]
struct ResultRow {
    id: String,
    accepted: bool,
    reason: Option<&'static str>,
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    std::io::stdin().take(262_145).read_to_end(&mut input)?;
    if input.len() > 262_144 {
        return Err("fixture exceeds limit".into());
    }
    let fixture: Fixture = serde_json::from_slice(&input)?;
    if fixture.cases.len() > 256 {
        return Err("too many fixture cases".into());
    }
    let mut rows = Vec::new();
    for case in fixture.cases {
        if case.id.is_empty()
            || case.id.len() > 80
            || !case
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err("invalid fixture ID".into());
        }
        Request {
            instruction: String::new(),
            transcript: case.original.clone(),
            context: String::new(),
        }
        .validate()?;
        let json = serde_json::to_vec(&Answer {
            text: case.candidate,
        })?;
        let result = parse_answer(&json, &case.original);
        rows.push(ResultRow {
            id: case.id,
            accepted: result.is_ok(),
            reason: result.err(),
        });
    }
    serde_json::to_writer(std::io::stdout().lock(), &rows)?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("Guard fixture validation failed");
        std::process::exit(1);
    }
}
