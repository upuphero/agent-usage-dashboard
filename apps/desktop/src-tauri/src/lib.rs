mod commands;
mod composition;
mod diagnostics;
mod export;
mod mapping;
mod profile;
mod runtime;
mod settings;
use std::sync::Arc;
use tauri::{Emitter, Manager};
pub fn diagnose_usage(output: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    diagnostics::run(output)
}
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let executable = std::env::current_exe()?;
            let executable_dir = executable
                .parent()
                .ok_or(usage_core::CoreError::SourceNotDetected)?;
            let (service, configs, settings) =
                composition::bootstrap(&app.path().app_data_dir()?, executable_dir)?;
            let runtime = runtime::Runtime::new_with_settings(
                service,
                configs,
                Arc::new(move |scan| {
                    let _ = handle.emit(usage_contracts::SCAN_EVENT, scan);
                }),
                Some(settings),
            );
            tauri::async_runtime::block_on(runtime.recover_interrupted_scans())?;
            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_api_info,
            commands::list_providers,
            commands::start_scan,
            commands::get_scan,
            commands::cancel_scan,
            commands::get_overview,
            commands::list_sessions,
            commands::export_usage,
            commands::get_settings,
            commands::update_settings,
            commands::choose_provider_directory
        ])
        .build(tauri::generate_context!())
        .expect("desktop initialization failed");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let runtime = handle.state::<Arc<runtime::Runtime>>().inner().clone();
            if !runtime.is_exit_ready() {
                api.prevent_exit();
                if runtime.begin_shutdown() {
                    let app = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        runtime.shutdown().await;
                        app.exit(0);
                    });
                }
            }
        }
    });
}
