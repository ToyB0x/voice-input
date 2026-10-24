//! Bounded, local-only Japanese ASR. No downloads or network clients.
//! Call from a worker thread: loading and inference are blocking operations.
mod resample;
pub use resample::prepare_audio;

use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const SAMPLE_RATE: u32 = 16_000;
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 60;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("model file is missing or unreadable")]
    ModelUnavailable,
    #[error("model path must be valid UTF-8")]
    ModelPath,
    #[error("could not load local model: {0}")]
    Model(String),
    #[error("a multilingual Whisper model is required for Japanese")]
    EnglishOnlyModel,
    #[error("audio must contain 1–60 seconds of finite mono PCM at 16000 Hz")]
    InvalidAudio,
    #[error("WAV must be mono, 16000 Hz, 16-bit PCM and at most 60 seconds")]
    InvalidWav,
    #[error("could not read WAV: {0}")]
    Wav(#[from] hound::Error),
    #[error("local inference failed: {0}")]
    Inference(String),
}

#[derive(Clone, Copy, Debug)]
pub enum Backend {
    Cpu,
    #[cfg(target_os = "macos")]
    Metal,
}

pub struct Transcriber {
    context: WhisperContext,
}

impl Transcriber {
    /// Models are supplied explicitly by the caller. Never fetches a missing model.
    pub fn load(path: &Path, backend: Backend) -> Result<Self, Error> {
        if !path.is_file() || std::fs::File::open(path).is_err() {
            return Err(Error::ModelUnavailable);
        }
        // No log_backend/tracing_backend features are enabled. The hooks discard
        // native diagnostics, including prompt tokens in debug builds.
        whisper_rs::install_logging_hooks();
        let mut options = WhisperContextParameters::default();
        options.use_gpu(!matches!(backend, Backend::Cpu));
        let context =
            WhisperContext::new_with_params(path.to_str().ok_or(Error::ModelPath)?, options)
                .map_err(|e| Error::Model(e.to_string()))?;
        if !context.is_multilingual() {
            return Err(Error::EnglishOnlyModel);
        }
        Ok(Self { context })
    }

    /// Model identity comes from loaded weights, not a user-controlled filename.
    pub fn model_name(&self) -> String {
        self.context
            .model_type_readable_str_lossy()
            .map(|name| name.into_owned())
            .unwrap_or_else(|_| "unknown".into())
    }

    /// Input is normalized mono PCM. Empty/short/oversized/NaN input is rejected.
    /// Digital silence bypasses decoding to avoid silence hallucinations.
    pub fn transcribe(&self, audio: &[f32]) -> Result<String, Error> {
        self.transcribe_with_hint(audio, None)
    }

    /// A bounded, per-call local hint. It never becomes cross-recording context.
    pub fn transcribe_with_hint(&self, audio: &[f32], hint: Option<&str>) -> Result<String, Error> {
        validate_audio(audio)?;
        let energy = audio.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / audio.len() as f64;
        if energy.sqrt() < 0.00001 {
            return Ok(String::new());
        }
        let mut state = self
            .context
            .create_state()
            .map_err(|e| Error::Inference(e.to_string()))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("ja"));
        params.set_translate(false);
        params.set_no_context(true);
        let hint = recognition_hint(hint.unwrap_or_default());
        if !hint.is_empty() {
            params.set_initial_prompt(&hint);
        }
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(
            std::thread::available_parallelism().map_or(1, |n| n.get().min(4)) as i32,
        );
        state
            .full(params, audio)
            .map_err(|e| Error::Inference(e.to_string()))?;
        let mut text = String::new();
        for segment in state.as_iter() {
            let part = segment
                .to_str()
                .map_err(|e| Error::Inference(e.to_string()))?;
            text.push_str(part);
        }
        Ok(text.trim().to_owned())
    }
}

/// Remove control characters (including NUL) before the C API; bound UTF-8 size.
/// Shared by AX capture and the evaluation harness so their prompt is identical.
pub fn recognition_hint(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        if result.len() + c.len_utf8() > 768 {
            break;
        }
        result.push(c);
    }
    result.trim().to_owned()
}

fn validate_audio(audio: &[f32]) -> Result<(), Error> {
    if !(SAMPLE_RATE as usize..=MAX_SAMPLES).contains(&audio.len())
        || audio.iter().any(|s| !s.is_finite() || s.abs() > 1.0)
    {
        return Err(Error::InvalidAudio);
    }
    Ok(())
}

/// Strict validation before allocation; no implicit resampling or channel mixing.
/// The recording adapter uses `prepare_audio` to supply the same PCM contract.
pub fn read_wav(path: &Path) -> Result<Vec<f32>, Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != SAMPLE_RATE
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
        || !(SAMPLE_RATE..=MAX_SAMPLES as u32).contains(&reader.duration())
    {
        return Err(Error::InvalidWav);
    }
    let audio = reader
        .samples::<i16>()
        .take(MAX_SAMPLES + 1)
        .map(|s| s.map(|v| f32::from(v) / 32768.0))
        .collect::<Result<Vec<_>, _>>()?;
    validate_audio(&audio)?;
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_bounded_unicode_and_c_safe() {
        assert_eq!(recognition_hint("  固有\0名詞\n "), "固有名詞");
        let hint = recognition_hint(&"😀".repeat(300));
        assert_eq!(hint.len(), 768);
        assert_eq!(hint.chars().count(), 192);
        assert!(recognition_hint("\0\n\t").is_empty());
    }

    #[test]
    fn rejects_unbounded_and_invalid_pcm() {
        for audio in [
            vec![],
            vec![0.0; 15_999],
            vec![0.0; MAX_SAMPLES + 1],
            vec![f32::NAN; 16_000],
            vec![f32::INFINITY; 16_000],
            vec![1.1; 16_000],
        ] {
            assert!(matches!(validate_audio(&audio), Err(Error::InvalidAudio)));
        }
        assert!(validate_audio(&vec![0.0; 16_000]).is_ok());
        assert!(validate_audio(&vec![-1.0; MAX_SAMPLES]).is_ok());
    }

    #[test]
    fn missing_model_is_actionable() {
        assert!(matches!(
            Transcriber::load(Path::new("/no-such-voice-input-model.bin"), Backend::Cpu),
            Err(Error::ModelUnavailable)
        ));
    }

    #[test]
    fn fixture_is_valid_pcm() {
        let samples = read_wav(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ja-oku.wav"
        )))
        .unwrap();
        assert_eq!(samples.len(), 12 * SAMPLE_RATE as usize);
    }
}
