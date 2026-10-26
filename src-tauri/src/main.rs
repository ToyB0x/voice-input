#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(any(target_os = "macos", test))]
mod audio_buffer;
#[cfg(target_os = "macos")]
mod recorder;
#[cfg(target_os = "macos")]
mod transcription;

#[cfg(target_os = "macos")]
mod context;
#[cfg(target_os = "macos")]
mod correction;
#[cfg(target_os = "macos")]
mod desktop;
#[cfg(target_os = "macos")]
mod insertion;
#[cfg(target_os = "macos")]
mod models;
#[cfg(target_os = "macos")]
mod ocr;
#[cfg(any(target_os = "macos", test))]
mod settings;

#[cfg(target_os = "macos")]
mod activity;
#[cfg(target_os = "macos")]
mod comparison;
#[cfg(target_os = "macos")]
mod lifecycle;
#[cfg(target_os = "macos")]
mod updater;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let inserter = insertion::Inserter::new();
    #[cfg(target_os = "macos")]
    let context = context::Reader::new();
    #[cfg(target_os = "macos")]
    let corrector = correction::Corrector::new();
    #[cfg(target_os = "macos")]
    let comparisons = comparison::Comparisons::new();
    #[cfg(target_os = "macos")]
    let updates = updater::Updates::new();
    #[cfg(target_os = "macos")]
    let activity = activity::Activity::default();
    #[cfg(target_os = "macos")]
    let builder = if updater::PUBLIC_KEY.trim().is_empty() {
        builder
    } else {
        builder.plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(updater::PUBLIC_KEY.trim())
                .build(),
        )
    };
    #[cfg(target_os = "macos")]
    let builder = builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    desktop::shortcut_event(app, shortcut, event.state);
                })
                .build(),
        )
        .manage(recorder::Recorder::new(
            inserter.clone(),
            context.clone(),
            corrector.clone(),
            comparisons.clone(),
            updates.clone(),
            activity.clone(),
        ))
        .manage(comparisons)
        .manage(updates)
        .manage(activity)
        .manage(lifecycle::Lifecycle::default())
        .manage(context)
        .manage(corrector)
        .manage(inserter)
        .setup(|app| {
            lifecycle::install_native_quit_hook(app.handle())?;
            desktop::setup(app)?;
            updater::start_automatic_checks(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            comparison::copy_comparison_report,
            comparison::comparison_models,
            comparison::comparison_plan,
            comparison::comparison_status,
            comparison::start_comparison,
            comparison::cancel_comparison,
            comparison::arm_comparison_context,
            comparison::copy_comparison_result,
            updater::update_status,
            updater::check_updates,
            updater::install_update,
            models::model_catalog,
            models::model_preparation_status,
            models::prepare_models,
            models::cancel_model_preparation,
            recorder::recording_status,
            recorder::recording_level,
            recorder::request_microphone_permission,
            recorder::start_recording,
            recorder::stop_recording,
            recorder::clear_recording,
            recorder::open_microphone_settings,
            recorder::load_model,
            recorder::transcribe_recording,
            recorder::copy_transcript,
            desktop::desktop_status,
            desktop::save_preferences,
            insertion::insertion_status,
            context::context_status,
            ocr::request_screen_permission,
            correction::correct_transcript,
            correction::correction_status,
            correction::copy_correction,
            correction::check_foundation_model,
            correction::copy_foundation_diagnostics,
            insertion::request_accessibility,
        ]);
    builder
        .build(tauri::generate_context!())
        .expect("failed to build Voice Input")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            match event {
                tauri::RunEvent::Reopen { .. } => desktop::show(app),
                tauri::RunEvent::ExitRequested { api, code, .. } => {
                    lifecycle::request_exit(app, api, code)
                }
                tauri::RunEvent::Exit => lifecycle::native_exit(),
                _ => {}
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
