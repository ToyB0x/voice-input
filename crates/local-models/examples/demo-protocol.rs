//! CI harness for the exact app prompt/parser; no inference or model downloads here.
#[path = "../../../src-tauri/src/demo_protocol.rs"]
mod demo_protocol;
use std::io::{self, Read};
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let v: serde_json::Value = serde_json::from_str(&input).unwrap();
    let raw = v["raw"].as_str().unwrap();
    let text = v["text"].as_str();
    let image = text.is_none();
    let mut out = serde_json::json!({"prompt":demo_protocol::prompt(raw,text).unwrap(),"schema":demo_protocol::schema(image),"config":demo_protocol::config(),"model":serde_json::from_str::<serde_json::Value>(demo_protocol::MODEL).unwrap()});
    out["backendArgs"] = serde_json::json!(demo_protocol::backend_args(
        v["useGpu"].as_bool().unwrap_or(false),
        image
    ));
    out["observationPrompt"] = demo_protocol::OBSERVATION_PROMPT.into();
    out["observationSchema"] = demo_protocol::observation_schema();
    out["residentBody"] =
        demo_protocol::resident_body(out["prompt"].clone(), demo_protocol::schema(false));
    if let Some(output) = v["output"].as_str() {
        let choice = if v["resident"] == true {
            demo_protocol::parse_resident(output)
        } else {
            demo_protocol::parse(output, image)
        }
        .unwrap();
        out["final"] = demo_protocol::apply(raw, choice["spelling"].as_str().unwrap())
            .unwrap()
            .into();
    }
    println!("{out}");
}
