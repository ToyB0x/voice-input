use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use local_correction::{MAX_INPUT, MAX_OUTPUT, Report, Request, parse_answer};
use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    num::NonZeroU32,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
    time::Instant,
};
static PHASE: AtomicU8 = AtomicU8::new(0);
const PHASES: [&str; 8] = [
    "input",
    "backend",
    "model",
    "prompt",
    "prefill",
    "generation",
    "validation",
    "output",
];
#[derive(Debug)]
struct Rejection(&'static str);
impl std::fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for Rejection {}
static OFFLOADED: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn native_log(
    _: llama_cpp_sys_2::ggml_log_level,
    text: *const c_char,
    _: *mut c_void,
) {
    if text.is_null() {
        return;
    }
    // Never forward native log text. Only collect a numeric offload signal.
    let bytes = unsafe { CStr::from_ptr(text) }.to_bytes();
    if let Ok(text) = std::str::from_utf8(bytes)
        && text.contains("offloaded ")
        && text.contains(" layers to GPU")
        && let Some(tail) = text.split("offloaded ").nth(1)
        && let Some(number) = tail.split('/').next().and_then(|s| s.parse::<u32>().ok())
    {
        OFFLOADED.store(number, Ordering::Relaxed);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || !["cpu", "metal"].contains(&args[1].as_str()) {
        return Err(
            "usage: local-correction /absolute/model.gguf cpu|metal (JSON on stdin)".into(),
        );
    }
    if !std::path::Path::new(&args[0]).is_absolute() {
        return Err("absolute model path required".into());
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut input)?;
    if input.len() > MAX_INPUT {
        return Err("correction input exceeds limit".into());
    }
    let request: Request =
        serde_json::from_slice(&input).map_err(|_| "invalid correction request")?;
    request.validate()?;
    let started = Instant::now();
    if request.transcript.trim().is_empty() {
        serde_json::to_writer(
            std::io::stdout(),
            &Report {
                text: String::new(),
                model: String::new(),
                backend: args[1].clone(),
                offloaded_layers: 0,
                elapsed_ms: 0,
            },
        )?;
        return Ok(());
    }
    unsafe {
        llama_cpp_sys_2::llama_log_set(Some(native_log), std::ptr::null_mut());
        llama_cpp_sys_2::ggml_log_set(Some(native_log), std::ptr::null_mut());
    }
    PHASE.store(1, Ordering::Relaxed);
    let backend = LlamaBackend::init()?;
    let metal = args[1] == "metal";
    if metal && !backend.supports_gpu_offload() {
        return Err("Metal is unavailable; choose CPU explicitly".into());
    }
    PHASE.store(2, Ordering::Relaxed);
    let model = LlamaModel::load_from_file(
        &backend,
        &args[0],
        &LlamaModelParams::default().with_n_gpu_layers(if metal { 999 } else { 0 }),
    )?;
    if model.meta_val_str("general.architecture")? != "qwen3" {
        return Err("only Qwen3 text models are supported".into());
    }
    let name = model.meta_val_str("general.name")?;
    let layers = OFFLOADED.load(Ordering::Relaxed);
    if (metal && layers == 0) || (!metal && layers != 0) {
        return Err("requested GPU offload did not occur".into());
    }
    PHASE.store(3, Ordering::Relaxed);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4)) as i32;
    let params = LlamaContextParams::default()
        .with_offload_kqv(metal)
        .with_op_offload(metal)
        .with_n_ctx(NonZeroU32::new(4096))
        .with_n_batch(2048)
        .with_n_ubatch(256)
        .with_n_threads(threads)
        .with_n_threads_batch(threads);
    let mut context = model.new_context(&backend, params)?;
    let vocab = model.vocab();
    let tokens = vocab.tokenize(request.prompt()?.as_bytes(), true, true);
    if tokens.is_empty() || tokens.len() > 3500 {
        return Err("correction prompt exceeds context budget".into());
    }
    PHASE.store(4, Ordering::Relaxed);
    let mut batch = LlamaBatch::new(2048, 1);
    // Decode in bounded chunks, preserving token positions across chunks.
    for (chunk_index, chunk) in tokens.chunks(2048).enumerate() {
        batch.clear();
        for (index, token) in chunk.iter().enumerate() {
            let position = chunk_index * 2048 + index;
            batch.add(*token, position as i32, &[0], position + 1 == tokens.len())?;
        }
        context.decode(&mut batch)?;
    }
    PHASE.store(5, Ordering::Relaxed);
    const GRAMMAR: &str = "root ::= \"{\" ws \"\\\"text\\\"\" ws \":\" ws string ws \"}\"\nws ::= [ \\t\\n\\r]*\nstring ::= \"\\\"\" char* \"\\\"\"\nchar ::= [^\"\\\\\\x00-\\x1F] | \"\\\\\" ([\"\\\\/bfnrt] | \"u\" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F])";
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::grammar(&model, GRAMMAR, "root")?,
        LlamaSampler::greedy(),
    ]);
    let mut generated = Vec::new();
    let mut ended = false;
    for position in tokens.len()..tokens.len() + 512 {
        if started.elapsed().as_secs() > 60 {
            return Err("correction deadline exceeded".into());
        }
        let token = sampler.sample(&context, -1);
        if vocab.is_eog(token) {
            ended = true;
            break;
        }
        generated.push(token);
        let bytes = vocab.detokenize(&generated, false, false);
        if bytes.len() > MAX_OUTPUT {
            return Err("correction output exceeds limit".into());
        }
        // Stop once a complete constrained JSON response is available.
        if serde_json::from_slice::<local_correction::Answer>(&bytes).is_ok() {
            ended = true;
            break;
        }
        batch.clear();
        batch.add(token, position as i32, &[0], true)?;
        context.decode(&mut batch)?;
    }
    if !ended {
        return Err("correction output was truncated".into());
    }
    PHASE.store(6, Ordering::Relaxed);
    let text = parse_answer(
        &vocab.detokenize(&generated, false, false),
        &request.transcript,
    )
    .map_err(Rejection)?;
    PHASE.store(7, Ordering::Relaxed);
    let report = Report {
        text,
        model: name,
        backend: args[1].clone(),
        offloaded_layers: layers,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, &report)?;
    output.write_all(b"\n")?;
    Ok(())
}
fn main() {
    // Rust bindings can panic with paths or source data. Suppress the panic hook
    // and report only the fixed phase, just as for returned native errors.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(run);
    if !matches!(&result, Ok(Ok(()))) {
        let rejection = match &result {
            Ok(Err(error)) => error.downcast_ref::<Rejection>(),
            _ => None,
        };
        let mut output = std::io::stdout().lock();
        let _ = serde_json::to_writer(
            &mut output,
            &serde_json::json!({
                "error": if rejection.is_some() { "rejected" } else { "runtime" },
                "stage": PHASES[usize::from(PHASE.load(Ordering::Relaxed))],
                "reason": rejection.map(|error| error.0),
            }),
        );
        let _ = output.write_all(b"\n");
        let _ = output.flush();
        eprintln!(
            "Local correction failed. Check model, backend and input limits; the original transcript is unchanged."
        );
        std::process::exit(1);
    }
}
