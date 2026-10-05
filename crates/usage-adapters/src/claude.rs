use crate::process::{io_error, COLLECTOR_VERSION, NORMALIZATION_VERSION};
use crate::ProcessRunner;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};
use usage_core::*;

const PROVIDER: &str = "ccusage.claude-code";
const PRODUCT: &str = "claude-code";

pub struct ClaudeCodeAdapter {
    runner: ProcessRunner,
    dataset: String,
    device: String,
    default_root: Option<PathBuf>,
    gate: tokio::sync::Mutex<()>,
}
impl ClaudeCodeAdapter {
    /// IDs must be persisted by the host; never derive them from a path or token count.
    /// root_path in SourceConfig overrides default_root; otherwise only ~/.claude is selected.
    pub fn new(
        runner: ProcessRunner,
        source_dataset_id: String,
        origin_device_id: String,
        default_root: Option<PathBuf>,
    ) -> Result<Self, CoreError> {
        if [&source_dataset_id, &origin_device_id]
            .iter()
            .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err(CoreError::InvalidQuery);
        }
        Ok(Self {
            runner,
            dataset: source_dataset_id,
            device: origin_device_id,
            default_root,
            gate: tokio::sync::Mutex::new(()),
        })
    }
    fn root(&self, config: &SourceConfig) -> Result<PathBuf, CoreError> {
        let root = config
            .root_path
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.default_root.clone())
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(|home| PathBuf::from(home).join(".claude"))
            })
            .ok_or(CoreError::SourceNotDetected)?;
        if !root.is_absolute()
            || root
                .to_str()
                .is_none_or(|s| s.contains(',') || s.trim() != s)
        {
            return Err(CoreError::InvalidQuery);
        }
        let root = if root.file_name().is_some_and(|n| n == "projects") {
            root.parent().ok_or(CoreError::InvalidQuery)?.to_path_buf()
        } else {
            root
        };
        let root = root.canonicalize().map_err(io_error)?;
        std::fs::read_dir(root.join("projects")).map_err(io_error)?;
        Ok(root)
    }
}
#[async_trait]
impl UsageSource for ClaudeCodeAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: PROVIDER.into(),
            product_id: PRODUCT.into(),
            display_name: "Claude Code".into(),
            capabilities: ProviderCapabilities {
                report_kinds: vec![ReportKind::Daily, ReportKind::Session],
                supported_dimensions: vec!["day".into(), "model".into(), "session".into()],
                supported_metrics: vec![
                    "input".into(),
                    "output".into(),
                    "cache".into(),
                    "cost".into(),
                    "total".into(),
                ],
                supports_date_session_intersection: false,
                supports_incremental_collection: false,
                supports_quota: false,
            },
        }
    }
    async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError> {
        match self.root(config) {
            Ok(_) => Ok(Detection {
                state: ProviderState::Ready,
                path_hint: Some("<configured-claude-root>/projects".into()),
            }),
            Err(CoreError::SourceNotDetected) => Ok(Detection {
                state: ProviderState::NotDetected,
                path_hint: None,
            }),
            Err(error) => Err(error),
        }
    }
    async fn collect(
        &self,
        request: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<CollectionBatch, CoreError> {
        if !request.config.enabled {
            return Err(CoreError::ProviderDisabled);
        }
        usage_core::application::validate_timezone(&request.timezone)?;
        cancellation.check()?;
        let _gate = self.gate.try_lock().map_err(|_| CoreError::ScanBusy)?;
        let root = self.root(&request.config)?;
        let started = Utc::now();
        let before = audit_async(root.clone(), cancellation.clone()).await?;
        let daily = self
            .runner
            .run_report(
                ReportKind::Daily,
                &root,
                &request.timezone,
                cancellation.clone(),
            )
            .await?;
        let sessions = self
            .runner
            .run_report(
                ReportKind::Session,
                &root,
                &request.timezone,
                cancellation.clone(),
            )
            .await?;
        let after = audit_async(root, cancellation.clone()).await?;
        if before != after {
            return Err(CoreError::CoverageIncomplete);
        }
        cancellation.check()?;
        let collected = Utc::now();
        let snapshots = [
            (&daily, ReportKind::Daily),
            (&sessions, ReportKind::Session),
        ]
        .into_iter()
        .map(|(bytes, kind)| {
            let mut snapshot = decode_report(
                bytes,
                kind,
                &request.timezone,
                &self.dataset,
                &self.device,
                started,
                collected,
            )?;
            // Upstream fills missing raw buckets with zero. Never mislabel those placeholders as exact.
            for row in &mut snapshot.rows {
                for (metric, missing) in [
                    &mut row.tokens.input_uncached,
                    &mut row.tokens.output_total,
                    &mut row.tokens.cache_write,
                    &mut row.tokens.cache_read,
                ]
                .into_iter()
                .zip(before.missing_buckets)
                {
                    if missing {
                        *metric = Metric::unavailable();
                    }
                }
                if before.missing_buckets.iter().any(|v| *v) {
                    row.tokens.total = Metric::unavailable();
                    row.cost.amount_usd = Metric::unavailable();
                }
            }
            if before.usage_lines > 0 && snapshot.rows.is_empty() {
                return Err(CoreError::CoverageIncomplete);
            }
            if before.missing_buckets.iter().any(|v| *v) {
                snapshot.warnings.push("SOURCE_METRICS_INCOMPLETE".into());
            }
            Ok(snapshot)
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
        let batch = CollectionBatch { snapshots };
        usage_core::application::validate_batch(&batch)?;
        Ok(batch)
    }
}

// Only typed, whitelisted report fields are decoded. Project paths/titles/body never enter Core or storage.
#[derive(Deserialize)]
struct RawReport {
    daily: Option<Vec<RawRow>>,
    sessions: Option<Vec<RawRow>>,
    totals: RawTotals,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTotals {
    total_tokens: u64,
    #[serde(default)]
    unpriced_models: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRow {
    date: Option<String>,
    session_id: Option<String>,
    first_activity: Option<String>,
    last_activity: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    total_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    total_cost: Option<serde_json::Number>,
    #[serde(default)]
    models_used: Vec<String>,
    #[serde(default)]
    model_breakdowns: Vec<RawModel>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawModel {
    model_name: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cost: Option<serde_json::Number>,
    reasoning_output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    #[serde(default)]
    missing_pricing: bool,
}
fn metric(value: Option<u64>) -> Metric<u64> {
    value.map(Metric::exact).unwrap_or_default()
}
fn timestamp(value: Option<&str>) -> Result<Option<DateTime<Utc>>, CoreError> {
    value
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|v| v.with_timezone(&Utc))
                .map_err(|_| CoreError::SchemaUnsupported)
        })
        .transpose()
}
fn cost(
    value: Option<&serde_json::Number>,
    missing_models: Vec<String>,
    all_missing: bool,
) -> Result<CostEstimate, CoreError> {
    let amount = if all_missing {
        None
    } else {
        value
            .map(|n| {
                Decimal::from_str(&n.to_string())
                    .or_else(|_| Decimal::from_scientific(&n.to_string()))
                    .map_err(|_| CoreError::SchemaUnsupported)
            })
            .transpose()?
    };
    Ok(CostEstimate {
        amount_usd: amount
            .map(|value| Metric {
                value: Some(value),
                accuracy: Accuracy::Estimated,
            })
            .unwrap_or_default(),
        pricing_version: Some(format!("{COLLECTOR_VERSION}-embedded-calculate")),
        pricing_as_of: None,
        missing_models,
    })
}

pub(crate) fn decode_report(
    bytes: &[u8],
    kind: ReportKind,
    timezone: &str,
    dataset: &str,
    device: &str,
    started: DateTime<Utc>,
    collected: DateTime<Utc>,
) -> Result<ReportSnapshot, CoreError> {
    let report: RawReport =
        serde_json::from_slice(bytes).map_err(|_| CoreError::SchemaUnsupported)?;
    let raw_rows = match kind {
        ReportKind::Daily => report.daily,
        ReportKind::Session => report.sessions,
    }
    .ok_or(CoreError::SchemaUnsupported)?;
    let mut rows = Vec::new();
    let mut warnings = BTreeSet::from([
        "REASONING_UNAVAILABLE".to_string(),
        "PRICING_DATE_UNKNOWN".into(),
        "REPORTS_READ_SEPARATELY".into(),
    ]);
    let mut parent_total = Some(0u64);
    for raw in raw_rows {
        let dimension = match kind {
            ReportKind::Daily => {
                let text = raw.date.as_deref().ok_or(CoreError::SchemaUnsupported)?;
                let day: NaiveDate = text.parse().map_err(|_| CoreError::SchemaUnsupported)?;
                if day.to_string() != text {
                    return Err(CoreError::SchemaUnsupported);
                }
                RowDimension::Day(day)
            }
            ReportKind::Session => {
                RowDimension::Session(raw.session_id.clone().ok_or(CoreError::SchemaUnsupported)?)
            }
        };
        let session_started_at = if kind == ReportKind::Session {
            timestamp(raw.first_activity.as_deref())?
        } else {
            None
        };
        let last_activity_at = if kind == ReportKind::Session {
            timestamp(raw.last_activity.as_deref())?
        } else {
            None
        };
        if session_started_at
            .zip(last_activity_at)
            .is_some_and(|(a, b)| a > b)
        {
            return Err(CoreError::SchemaUnsupported);
        }
        let mut missing_models: Vec<_> = raw
            .model_breakdowns
            .iter()
            .filter(|m| m.missing_pricing)
            .map(|m| m.model_name.clone())
            .collect();
        missing_models.extend(
            raw.models_used
                .iter()
                .filter(|m| report.totals.unpriced_models.contains(m))
                .cloned(),
        );
        missing_models.sort();
        missing_models.dedup();
        if !missing_models.is_empty() {
            warnings.insert("MISSING_PRICING".into());
        }
        let all_missing = !missing_models.is_empty()
            && raw
                .models_used
                .iter()
                .chain(raw.model_breakdowns.iter().map(|model| &model.model_name))
                .all(|m| missing_models.contains(m));
        let tokens = TokenMetrics {
            input_uncached: metric(raw.input_tokens),
            output_total: metric(raw.output_tokens),
            cache_write: metric(raw.cache_creation_tokens),
            cache_read: metric(raw.cache_read_tokens),
            output_reasoning: metric(raw.reasoning_output_tokens),
            total: metric(raw.total_tokens),
        };
        parent_total = parent_total
            .zip(raw.total_tokens)
            .map(|(a, b)| a.checked_add(b).ok_or(CoreError::Overflow))
            .transpose()?;
        rows.push(ReportRow {
            key: RowKey {
                dimension: dimension.clone(),
                model_id: None,
            },
            model_vendor: None,
            tokens,
            cost: cost(raw.total_cost.as_ref(), missing_models, all_missing)?,
            session_started_at,
            last_activity_at,
        });
        if raw.model_breakdowns.is_empty() {
            warnings.insert("MODEL_BREAKDOWN_UNAVAILABLE".into());
        }
        for model in raw.model_breakdowns {
            let missing =
                model.missing_pricing || report.totals.unpriced_models.contains(&model.model_name);
            let model_id = model.model_name;
            rows.push(ReportRow {
                key: RowKey {
                    dimension: dimension.clone(),
                    model_id: Some(model_id.clone()),
                },
                model_vendor: model_id.starts_with("claude-").then(|| "anthropic".into()),
                tokens: TokenMetrics {
                    input_uncached: metric(model.input_tokens),
                    output_total: metric(model.output_tokens),
                    cache_write: metric(model.cache_creation_tokens),
                    cache_read: metric(model.cache_read_tokens),
                    output_reasoning: metric(model.reasoning_output_tokens),
                    total: metric(model.total_tokens),
                },
                cost: cost(
                    model.cost.as_ref(),
                    if missing { vec![model_id] } else { vec![] },
                    missing,
                )?,
                // Activity is reported only for the parent session, not per-model history.
                session_started_at: None,
                last_activity_at: None,
            });
        }
    }
    if parent_total.is_some_and(|v| v != report.totals.total_tokens) {
        return Err(CoreError::SchemaUnsupported);
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    let dates: Vec<_> = rows
        .iter()
        .filter_map(|r| match r.key.dimension {
            RowDimension::Day(day) => Some(day),
            _ => None,
        })
        .collect();
    let mut batch = CollectionBatch {
        snapshots: vec![ReportSnapshot {
            key: SnapshotKey {
                product_id: PRODUCT.into(),
                source_dataset_id: dataset.into(),
                report_kind: kind,
                timezone: timezone.into(),
                scope: QueryScope::Standard,
            },
            provider_id: PROVIDER.into(),
            origin_device_id: device.into(),
            revision: 0,
            collected_at: collected,
            collection_started_at: started,
            collector_version: COLLECTOR_VERSION.into(),
            normalization_version: NORMALIZATION_VERSION.into(),
            coverage: Coverage {
                state: CoverageState::Complete,
                range: None,
                observed_from: dates.iter().min().copied(),
                observed_until: dates.iter().max().and_then(|day| day.succ_opt()),
            },
            warnings: warnings.into_iter().collect(),
            rows,
        }],
    };
    // Reuse Core's public normalization and invariants; no competing aggregation or deduplication.
    usage_core::application::normalize_batch(&mut batch)?;
    usage_core::application::validate_batch(&batch)?;
    Ok(batch.snapshots.remove(0))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct SourceAudit {
    files: BTreeMap<PathBuf, [u8; 32]>,
    missing_buckets: [bool; 4],
    usage_lines: u64,
}
async fn audit_async(
    root: PathBuf,
    cancellation: CancellationToken,
) -> Result<SourceAudit, CoreError> {
    tokio::task::spawn_blocking(move || audit_source(&root, &cancellation))
        .await
        .map_err(|_| CoreError::CollectionFailed)?
}
fn audit_source(root: &Path, cancellation: &CancellationToken) -> Result<SourceAudit, CoreError> {
    let mut audit = SourceAudit::default();
    let mut directories = vec![(root.join("projects"), 0)];
    while let Some((directory, depth)) = directories.pop() {
        cancellation.check()?;
        if depth > 32 {
            return Err(CoreError::CoverageIncomplete);
        }
        if std::fs::symlink_metadata(&directory)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(CoreError::CoverageIncomplete);
        }
        for entry in std::fs::read_dir(&directory).map_err(io_error)? {
            cancellation.check()?;
            let entry = entry.map_err(io_error)?;
            let file_type = entry.file_type().map_err(io_error)?;
            if file_type.is_symlink() {
                return Err(CoreError::CoverageIncomplete);
            }
            let path = entry.path();
            if file_type.is_dir() {
                directories.push((path, depth + 1));
                continue;
            }
            if !file_type.is_file()
                || path
                    .extension()
                    .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            if audit.files.len() >= 100_000 {
                return Err(CoreError::CoverageIncomplete);
            }
            let mut reader = BufReader::new(std::fs::File::open(&path).map_err(io_error)?);
            let mut hash = Sha256::new();
            loop {
                cancellation.check()?;
                let line = bounded_line(&mut reader)?;
                if line.is_empty() {
                    break;
                }
                hash.update(&line);
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let value: serde_json::Value =
                    serde_json::from_slice(&line).map_err(|_| CoreError::CoverageIncomplete)?;
                let Some(usage) = value.get("message").and_then(|m| m.get("usage")) else {
                    if value.get("usage").is_some() {
                        return Err(CoreError::SchemaUnsupported);
                    }
                    continue;
                };
                if !usage.is_object()
                    || value
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .is_none_or(|v| DateTime::parse_from_rfc3339(v).is_err())
                {
                    return Err(CoreError::SchemaUnsupported);
                }
                // Fixed ccusage loaders use a compact usage marker. Whitespace variants can be silently skipped.
                if !line
                    .windows(b"\"usage\":{".len())
                    .any(|w| w == b"\"usage\":{")
                {
                    return Err(CoreError::SchemaUnsupported);
                }
                for (i, key) in [
                    "input_tokens",
                    "output_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                ]
                .iter()
                .enumerate()
                {
                    match usage.get(key) {
                        None => audit.missing_buckets[i] = true,
                        Some(value) if value.as_u64().is_some_and(|v| v <= i64::MAX as u64) => {}
                        _ => return Err(CoreError::SchemaUnsupported),
                    }
                }
                audit.usage_lines = audit
                    .usage_lines
                    .checked_add(1)
                    .ok_or(CoreError::Overflow)?;
            }
            audit.files.insert(
                path.strip_prefix(root)
                    .map_err(|_| CoreError::CoverageIncomplete)?
                    .to_path_buf(),
                hash.finalize().into(),
            );
        }
    }
    Ok(audit)
}
fn bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>, CoreError> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().map_err(io_error)?;
        if buffer.is_empty() {
            return Ok(line);
        }
        let length = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buffer.len(), |i| i + 1);
        if line.len() + length > 4 * 1024 * 1024 {
            return Err(CoreError::OutputLimitExceeded);
        }
        let done = buffer[length - 1] == b'\n';
        line.extend_from_slice(&buffer[..length]);
        reader.consume(length);
        if done {
            return Ok(line);
        }
    }
}

#[cfg(test)]
mod tests;
