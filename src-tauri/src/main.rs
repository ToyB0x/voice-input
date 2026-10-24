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
mod ocr;
#[cfg(any(target_os = "macos", test))]
mod settings;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let inserter = insertion::Inserter::new();
    #[cfg(target_os = "macos")]
    let context = context::Reader::new();
    #[cfg(target_os = "macos")]
    let corrector = correction::Corrector::new();
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
        ))
        .manage(context)
        .manage(corrector)
        .manage(inserter)
        .setup(desktop::setup)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            recorder::recording_status,
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
            insertion::request_accessibility,
        ]);
    builder
        .build(tauri::generate_context!())
        .expect("failed to build Voice Input")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                desktop::show(app);
            }
            #[cfg(target_os = "macos")]
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                use tauri::Manager;
                app.state::<context::Reader>().cancel();
                app.state::<correction::Corrector>().shutdown();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
