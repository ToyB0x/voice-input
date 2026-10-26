//! Numeric diagnostics only. No source text, paths, or persistent logging.
use serde::{Deserialize, Serialize};

/// Non-overlapping phases of a successful inference, in monotonic milliseconds.
/// RSS is the process lifetime high-water mark, not GPU memory or a stage delta.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Performance {
    pub backend_ms: u64,
    pub model_load_ms: u64,
    pub context_ms: u64,
    pub prefill_ms: u64,
    pub generation_ms: u64,
    pub validation_ms: u64,
    pub input_tokens: u32,
    pub generated_tokens: u32,
    pub peak_rss_bytes: Option<u64>,
}

pub fn peak_rss_bytes() -> Option<u64> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // getrusage initializes the entire output on success; never read on error.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        let rss = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).ok()?;
        #[cfg(target_os = "linux")]
        {
            rss.checked_mul(1024) // Linux reports KiB, macOS reports bytes.
        }
        #[cfg(target_os = "macos")]
        {
            Some(rss)
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    None
}
