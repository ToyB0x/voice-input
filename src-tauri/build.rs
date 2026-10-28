fn main() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=VOICE_INPUT_SOURCE_REVISION={revision}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    if let Ok(output) = std::process::Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        && let Ok(reference) = String::from_utf8(output.stdout)
    {
        println!("cargo:rerun-if-changed=../.git/{}", reference.trim());
    }

    println!("cargo:rerun-if-env-changed=VOICE_INPUT_UPDATER_PUBLIC_KEY");
    let manifest = tauri_build::AppManifest::new().commands(&[
        "start_demo",
        "demo_status",
        "start_context_lab",
        "context_lab_status",
        "context_lab_report",
        "copy_context_lab_report",
        "copy_comparison_report",
        "comparison_models",
        "comparison_plan",
        "comparison_status",
        "start_comparison",
        "start_correction_selftest",
        "cancel_comparison",
        "arm_comparison_context",
        "copy_comparison_result",
        "update_status",
        "check_updates",
        "install_update",
        "model_catalog",
        "model_preparation_status",
        "prepare_models",
        "apply_usage_preset",
        "cancel_model_preparation",
        "recording_status",
        "recording_level",
        "request_microphone_permission",
        "start_recording",
        "stop_recording",
        "clear_recording",
        "open_microphone_settings",
        "load_model",
        "transcribe_recording",
        "copy_transcript",
        "copy_final_transcript",
        "desktop_status",
        "save_preferences",
        "insertion_status",
        "context_status",
        "request_screen_permission",
        "correct_transcript",
        "correction_status",
        "copy_correction",
        "check_foundation_model",
        "copy_foundation_diagnostics",
        "request_accessibility",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application");
}
