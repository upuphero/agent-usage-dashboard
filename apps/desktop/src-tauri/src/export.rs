use crate::{mapping, runtime::Runtime};
use std::sync::Arc;
use usage_contracts as api;
use usage_core as core;

pub async fn create(
    runtime: &Runtime,
    request: api::ExportRequest,
) -> Result<Vec<u8>, api::ApiError> {
    let query = mapping::overview_query(&request.query).map_err(mapping::error)?;
    match request.format {
        api::ExportFormat::Csv => {
            let overview = runtime
                .service
                .get_overview(query)
                .await
                .map_err(mapping::error)?;
            let mut csv = String::from("bucket_start,total_tokens,total_accuracy,total_known_rows,total_missing_rows,api_equivalent_estimate_usd,cost_accuracy,cost_missing_rows\r\n");
            for (date, usage) in overview.buckets {
                csv.push_str(
                    &[
                        date.to_string(),
                        usage
                            .tokens
                            .total
                            .metric
                            .value
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        format!("{:?}", usage.tokens.total.metric.accuracy).to_lowercase(),
                        usage.tokens.total.known_rows.to_string(),
                        usage.tokens.total.missing_rows.to_string(),
                        usage
                            .cost
                            .amount_usd
                            .metric
                            .value
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        format!("{:?}", usage.cost.amount_usd.metric.accuracy).to_lowercase(),
                        usage.cost.amount_usd.missing_rows.to_string(),
                    ]
                    .into_iter()
                    .map(|s| csv_cell(&s))
                    .collect::<Vec<_>>()
                    .join(","),
                );
                csv.push_str("\r\n");
            }
            Ok(csv.into_bytes())
        }
        api::ExportFormat::Json => {
            if !query.model_ids.is_empty() {
                return Err(mapping::error(core::CoreError::UnsupportedFilter));
            }
            // Validate provider IDs using the same use case as Overview, without importing aggregation into this layer.
            runtime
                .service
                .get_overview(query.clone())
                .await
                .map_err(mapping::error)?;
            let snapshots = runtime
                .service
                .repository
                .load_snapshots(core::SnapshotFilter {
                    timezone: Some(query.timezone),
                    ..Default::default()
                })
                .await
                .map_err(mapping::error)?;
            let mut archive = api::UsageArchive {
                archive_schema_version: "1.0.0".into(),
                app_version: env!("CARGO_PKG_VERSION").into(),
                exported_at: runtime.service.clock.now().to_rfc3339(),
                snapshots: Vec::new(),
            };
            for snapshot in snapshots.into_iter().filter(|s| {
                s.key.scope == core::QueryScope::Standard
                    && (query.provider_ids.is_empty()
                        || query.provider_ids.contains(&s.provider_id))
            }) {
                let mut rows = Vec::new();
                for row in snapshot.rows {
                    let mut usage = core::Aggregate::default();
                    core::application::add_row(&mut usage, &row).map_err(mapping::error)?;
                    let (date, session_id) = match row.key.dimension {
                        core::RowDimension::Day(date) => (Some(date.to_string()), None),
                        core::RowDimension::Session(id) => (None, Some(id)),
                    };
                    rows.push(api::ArchiveRow {
                        date,
                        session_id,
                        model_id: row.key.model_id,
                        model_vendor: row.model_vendor,
                        usage: mapping::aggregate(&usage),
                        started_at: row.session_started_at.map(|d| d.to_rfc3339()),
                        last_activity_at: row.last_activity_at.map(|d| d.to_rfc3339()),
                    });
                }
                archive.snapshots.push(api::ArchiveSnapshot {
                    provider_id: snapshot.provider_id,
                    product_id: snapshot.key.product_id,
                    source_dataset_id: snapshot.key.source_dataset_id,
                    origin_device_id: snapshot.origin_device_id,
                    report_kind: match snapshot.key.report_kind {
                        core::ReportKind::Daily => api::ReportKind::Daily,
                        core::ReportKind::Session => api::ReportKind::Session,
                    },
                    timezone: snapshot.key.timezone,
                    scope: api::ArchiveScope {
                        kind: "standard".into(),
                        range: None,
                        model_ids: vec![],
                    },
                    revision: snapshot.revision.to_string(),
                    collected_at: snapshot.collected_at.to_rfc3339(),
                    collection_started_at: snapshot.collection_started_at.to_rfc3339(),
                    collector_version: snapshot.collector_version,
                    normalization_version: snapshot.normalization_version,
                    coverage: mapping::coverage(snapshot.coverage),
                    warnings: snapshot.warnings,
                    rows,
                });
            }
            serde_json::to_vec_pretty(&archive).map_err(|_| export_error())
        }
    }
}
fn export_error() -> api::ApiError {
    api::ApiError {
        api_version: api::API_VERSION.into(),
        code: api::ErrorCode::ExportFailed,
        message: "Could not save export".into(),
        retryable: true,
    }
}
pub fn csv_cell(value: &str) -> String {
    let trimmed = value.trim_start();
    let safe = if trimmed
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r' | '\n'))
        || value
            .chars()
            .next()
            .is_some_and(|c| matches!(c, '\t' | '\r' | '\n'))
    {
        format!("'{value}")
    } else {
        value.into()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
pub async fn save(
    app: tauri::AppHandle,
    runtime: Arc<Runtime>,
    request: api::ExportRequest,
) -> Result<api::ExportResult, api::ApiError> {
    use tauri_plugin_dialog::DialogExt;
    let format = request.format;
    let bytes = create(&runtime, request).await?;
    let size = bytes.len().to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let filename = if format == api::ExportFormat::Json {
        "usage.aiusage.json"
    } else {
        "usage.csv"
    }
    .to_owned();
    let suggested_filename = filename.clone();
    // Runs outside the webview thread; only the native dialog supplies a path. No file/shell permission in UI.
    let worker = tokio::task::spawn_blocking(move || {
        let destination = app
            .dialog()
            .file()
            .set_file_name(filename)
            .blocking_save_file()
            .ok_or_else(|| mapping::error(core::CoreError::Cancelled))?;
        let path = destination.into_path().map_err(|_| export_error())?;
        std::fs::write(path, bytes).map_err(|_| export_error())
    });
    worker.await.map_err(|_| export_error())??;
    Ok(api::ExportResult {
        api_version: api::API_VERSION.into(),
        export_id: id,
        suggested_filename,
        media_type: if format == api::ExportFormat::Json {
            "application/json"
        } else {
            "text/csv"
        }
        .into(),
        byte_length: size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_quotes_and_neutralizes_formulas() {
        assert_eq!(csv_cell("=1+1"), "\"'=1+1\"");
        assert_eq!(csv_cell("  @SUM(A1)"), "\"'  @SUM(A1)\"");
        assert_eq!(csv_cell("a,\"b\""), "\"a,\"\"b\"\"\"");
    }
}
