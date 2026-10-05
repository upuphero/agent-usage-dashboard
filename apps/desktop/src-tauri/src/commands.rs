use crate::{mapping, runtime::Runtime};
use std::sync::Arc;
use tauri::State;
use usage_contracts as api;

#[tauri::command]
pub fn get_api_info(state: State<'_, Arc<Runtime>>) -> api::ApiInfo {
    let mut capabilities = vec![
        "overview".into(),
        "sessions".into(),
        "scan-polling".into(),
        "scan-events".into(),
        "export-json-full-history".into(),
        "export-csv".into(),
    ];
    if state.service.sources.is_empty() {
        capabilities.push("adapter-integration-pending".into());
    }
    if state.settings_available() {
        capabilities.extend([
            "settings-read".into(),
            "settings-write".into(),
            "source-directory-selection".into(),
        ]);
    }
    api::ApiInfo {
        api_version: api::API_VERSION.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        capabilities,
    }
}
#[tauri::command]
pub async fn list_providers(
    state: State<'_, Arc<Runtime>>,
) -> Result<Vec<api::ProviderSummary>, api::ApiError> {
    state
        .list_providers()
        .await
        .map(|items| items.into_iter().map(mapping::provider).collect())
        .map_err(mapping::error)
}
#[tauri::command]
pub async fn start_scan(
    state: State<'_, Arc<Runtime>>,
    request: api::StartScanRequest,
) -> Result<api::StartScanResult, api::ApiError> {
    state
        .inner()
        .start_scan(request)
        .await
        .map_err(mapping::error)
}
#[tauri::command]
pub async fn get_scan(
    state: State<'_, Arc<Runtime>>,
    request: api::JobRequest,
) -> Result<api::ScanSummary, api::ApiError> {
    state
        .get_scan(&request.job_id)
        .await
        .map(mapping::scan)
        .map_err(mapping::error)
}
#[tauri::command]
pub async fn cancel_scan(
    state: State<'_, Arc<Runtime>>,
    request: api::JobRequest,
) -> Result<(), api::ApiError> {
    state
        .cancel_scan(&request.job_id)
        .await
        .map_err(mapping::error)
}
#[tauri::command]
pub async fn get_overview(
    state: State<'_, Arc<Runtime>>,
    request: api::OverviewQuery,
) -> Result<api::OverviewResult, api::ApiError> {
    let query = mapping::overview_query(&request).map_err(mapping::error)?;
    state
        .service
        .get_overview(query)
        .await
        .map(|value| mapping::overview(value, request))
        .map_err(mapping::error)
}
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, Arc<Runtime>>,
    request: api::SessionQuery,
) -> Result<api::SessionPage, api::ApiError> {
    let query = mapping::session_query(&request).map_err(mapping::error)?;
    state
        .service
        .list_sessions(query)
        .await
        .map(mapping::sessions)
        .map_err(mapping::error)
}

#[tauri::command]
pub async fn export_usage(
    app: tauri::AppHandle,
    state: State<'_, Arc<Runtime>>,
    request: api::ExportRequest,
) -> Result<api::ExportResult, api::ApiError> {
    crate::export::save(app, state.inner().clone(), request).await
}

#[tauri::command]
pub async fn get_settings(
    state: State<'_, Arc<Runtime>>,
) -> Result<api::SettingsResult, api::ApiError> {
    state.get_settings().await
}
#[tauri::command]
pub async fn update_settings(
    state: State<'_, Arc<Runtime>>,
    request: api::UpdateSettingsRequest,
) -> Result<api::SettingsResult, api::ApiError> {
    state.update_settings(request).await
}
#[tauri::command]
pub async fn choose_provider_directory(
    app: tauri::AppHandle,
    state: State<'_, Arc<Runtime>>,
    request: api::ChooseProviderDirectoryRequest,
) -> Result<api::ChooseProviderDirectoryResult, api::ApiError> {
    use tauri_plugin_dialog::DialogExt;
    state
        .service
        .source(&request.provider_id)
        .map_err(mapping::error)?;
    if !state.settings_available() {
        return Err(mapping::error(usage_core::CoreError::UnsupportedFilter));
    }
    let title = match request.provider_id.as_str() {
        "ccusage.codex" => "选择 Codex 数据目录（.codex 或 sessions）",
        "ccusage.antigravity" => "选择 Antigravity 数据目录或 conversations",
        _ => "选择 Claude 配置目录或 projects 目录",
    };
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .map_err(|_| mapping::error(usage_core::CoreError::Storage))?;
    let directory = match picked {
        Some(path) => Some(
            state
                .remember_directory(
                    &request.provider_id,
                    path.into_path()
                        .map_err(|_| mapping::error(usage_core::CoreError::InvalidQuery))?,
                )
                .await?,
        ),
        None => None,
    };
    Ok(api::ChooseProviderDirectoryResult {
        api_version: api::API_VERSION.into(),
        provider_id: request.provider_id,
        directory,
    })
}
