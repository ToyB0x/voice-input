//! Opt-in fixed stage markers only; never log audio, paths or recognized text.
pub(crate) fn diagnostic(stage: &'static str) {
    if std::env::var_os("VOICE_INPUT_LIFECYCLE_DIAGNOSTICS").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        use std::io::Write;
        // A closed launcher pipe must never turn normal exit into a panic.
        let _ = writeln!(std::io::stderr().lock(), "voice-input:lifecycle:{stage}");
    }
}
