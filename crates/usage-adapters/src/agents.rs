use crate::{claude::decode_report, process::io_error, usage_snapshot, AgentKind, ProcessRunner};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use usage_core::*;

pub struct AgentAdapter {
    kind: AgentKind,
    runner: ProcessRunner,
    dataset: String,
    device: String,
    gate: tokio::sync::Mutex<()>,
}
impl AgentAdapter {
    pub fn new(
        kind: AgentKind,
        runner: ProcessRunner,
        dataset: String,
        device: String,
    ) -> Result<Self, CoreError> {
        if [&dataset, &device]
            .iter()
            .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err(CoreError::InvalidQuery);
        }
        Ok(Self {
            kind,
            runner,
            dataset,
            device,
            gate: tokio::sync::Mutex::new(()),
        })
    }
    fn roots(&self, config: &SourceConfig) -> Result<Vec<PathBuf>, CoreError> {
        if let Some(root) = &config.root_path {
            return Ok(vec![validate_agent_directory(self.kind, Path::new(root))?]);
        }
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or(CoreError::SourceNotDetected)?;
        let home = PathBuf::from(home);
        let candidates = match self.kind {
            AgentKind::Codex => vec![home.join(".codex")],
            AgentKind::Antigravity => [
                ".gemini/antigravity",
                ".gemini/antigravity-cli",
                ".gemini/antigravity-ide",
                ".gemini/antigravity-backup",
                ".config/antigravity",
            ]
            .into_iter()
            .map(|path| home.join(path))
            .collect(),
        };
        let roots = candidates
            .into_iter()
            .filter(|path| path.is_dir())
            .map(|path| validate_agent_directory(self.kind, &path))
            .collect::<Result<Vec<_>, _>>()?;
        if roots.is_empty() {
            return Err(CoreError::SourceNotDetected);
        }
        Ok(roots)
    }
}
pub fn validate_agent_directory(kind: AgentKind, path: &Path) -> Result<PathBuf, CoreError> {
    if !path.is_absolute()
        || path
            .to_str()
            .is_none_or(|text| text.contains(',') || text.contains('\0'))
    {
        return Err(CoreError::InvalidQuery);
    }
    let root = if path.file_name().is_some_and(|name| match kind {
        AgentKind::Codex => name == "sessions" || name == "archived_sessions",
        AgentKind::Antigravity => name == "conversations",
    }) {
        path.parent().ok_or(CoreError::InvalidQuery)?
    } else {
        path
    };
    let root = root.canonicalize().map_err(io_error)?;
    std::fs::read_dir(&root).map_err(io_error)?;
    Ok(root)
}
#[async_trait]
impl UsageSource for AgentAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: self.kind.provider().into(),
            product_id: self.kind.product().into(),
            display_name: self.kind.display_name().into(),
            capabilities: ProviderCapabilities {
                report_kinds: vec![ReportKind::Daily, ReportKind::Session],
                supported_dimensions: vec!["day".into(), "model".into(), "session".into()],
                supported_metrics: vec![
                    "input".into(),
                    "output".into(),
                    "cache".into(),
                    "total".into(),
                    "cost".into(),
                    "reasoning".into(),
                ],
                supports_date_session_intersection: false,
                supports_incremental_collection: false,
                supports_quota: false,
            },
        }
    }
    async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError> {
        match self.roots(config) {
            Ok(_) => Ok(Detection {
                state: ProviderState::Ready,
                path_hint: Some(
                    match self.kind {
                        AgentKind::Codex => "本机 Codex sessions / archived_sessions",
                        AgentKind::Antigravity => "本机 Antigravity SQLite conversations（.db）",
                    }
                    .into(),
                ),
            }),
            Err(CoreError::SourceNotDetected) => Ok(Detection {
                state: ProviderState::NotDetected,
                path_hint: None,
            }),
            Err(error) => Err(error),
        }
    }
    async fn inspect(
        &self,
        request: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<SourceObservation, CoreError> {
        if !request.config.enabled {
            return Err(CoreError::ProviderDisabled);
        }
        let roots = self.roots(&request.config)?;
        let dataset = self.dataset.clone();
        let kind = self.kind;
        tokio::task::spawn_blocking(move || {
            crate::changes::inspect(
                crate::changes::Layout::Agent(kind),
                &roots,
                &dataset,
                &request.timezone,
                &format!("{}-1|{}", kind.product(), crate::process::COLLECTOR_VERSION),
                &cancellation,
            )
        })
        .await
        .map_err(|_| CoreError::CollectionFailed)?
    }
    fn watch(
        &self,
        config: &SourceConfig,
        changed: ChangeCallback,
    ) -> Result<Box<dyn SourceWatch>, CoreError> {
        if !config.enabled {
            return Err(CoreError::ProviderDisabled);
        }
        crate::changes::watch(
            crate::changes::Layout::Agent(self.kind),
            self.roots(config)?,
            changed,
        )
    }
    async fn collect(
        &self,
        request: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<CollectionBatch, CoreError> {
        if !request.config.enabled {
            return Err(CoreError::ProviderDisabled);
        }
        application::validate_timezone(&request.timezone)?;
        let _gate = self.gate.try_lock().map_err(|_| CoreError::ScanBusy)?;
        let roots = self.roots(&request.config)?;
        let started = Utc::now();
        let kind = self.kind;
        let cancel = cancellation.clone();
        let captured =
            tokio::task::spawn_blocking(move || usage_snapshot::capture(kind, &roots, &cancel))
                .await
                .map_err(|_| CoreError::CollectionFailed)??;
        let daily = self
            .runner
            .run_agent_report(
                kind,
                ReportKind::Daily,
                &captured.roots,
                &request.timezone,
                cancellation.clone(),
            )
            .await?;
        let session = self
            .runner
            .run_agent_report(
                kind,
                ReportKind::Session,
                &captured.roots,
                &request.timezone,
                cancellation.clone(),
            )
            .await?;
        cancellation.check()?;
        let collected = Utc::now();
        let mut snapshots = vec![];
        for (bytes, report_kind) in [(&daily, ReportKind::Daily), (&session, ReportKind::Session)] {
            let normalized = normalize_report(kind, bytes, report_kind)?;
            let mut snapshot = decode_report(
                &serde_json::to_vec(&normalized).map_err(|_| CoreError::InvalidData)?,
                report_kind,
                &request.timezone,
                &self.dataset,
                &self.device,
                started,
                collected,
            )?;
            snapshot.provider_id = kind.provider().into();
            snapshot.key.product_id = kind.product().into();
            snapshot.normalization_version = format!("{}-1", kind.product());
            snapshot
                .warnings
                .retain(|warning| warning != "REASONING_UNAVAILABLE");
            snapshot.warnings.extend(captured.warnings.clone());
            snapshot.warnings.push("USAGE_METADATA_SNAPSHOT".into());
            if kind == AgentKind::Codex {
                snapshot
                    .warnings
                    .push("CODEX_UNCLASSIFIED_TIER_STANDARD_ESTIMATE".into());
                snapshot.warnings.push("MODEL_COST_UNAVAILABLE".into());
                if captured.nonzero_usage && snapshot.rows.is_empty() {
                    return Err(CoreError::CoverageIncomplete);
                }
            }
            for row in &mut snapshot.rows {
                if let Some(model) = &row.key.model_id {
                    row.model_vendor = if model.starts_with("gpt-") || model.starts_with("codex-") {
                        Some("openai".into())
                    } else if model.starts_with("gemini-") {
                        Some("google".into())
                    } else {
                        row.model_vendor.clone()
                    };
                }
                if kind == AgentKind::Codex {
                    if captured.missing[0] || captured.missing[1] {
                        row.tokens.input_uncached = Metric::unavailable();
                    }
                    if captured.missing[1] {
                        row.tokens.cache_read = Metric::unavailable();
                    }
                    if captured.missing[2] {
                        row.tokens.cache_write = Metric::unavailable();
                    }
                    if captured.missing[3] {
                        row.tokens.output_total = Metric::unavailable();
                    }
                    if captured.missing[4] {
                        row.tokens.output_reasoning = Metric::unavailable();
                    }
                    if captured.missing[0] || captured.missing[3] {
                        row.tokens.total = Metric::unavailable();
                    } else if captured.missing[5] && row.tokens.total.value.is_some() {
                        row.tokens.total.accuracy = Accuracy::Derived;
                    }
                    if captured.missing[0] || captured.missing[1] || captured.missing[3] {
                        row.cost.amount_usd = Metric::unavailable();
                    }
                    row.cost.pricing_version =
                        Some("ccusage-20.0.26-embedded-recorded-tier-standard-fallback".into());
                } else {
                    if row.tokens.output_reasoning.value.is_some() {
                        row.tokens.output_reasoning.accuracy = Accuracy::Derived;
                        row.tokens.output_total.accuracy = Accuracy::Derived;
                    }
                    if row.key.model_id.is_some() && row.tokens.output_total.value.is_none() {
                        snapshot
                            .warnings
                            .push("MODEL_OUTPUT_BREAKDOWN_UNAVAILABLE".into());
                    }
                }
            }
            if kind == AgentKind::Codex && captured.missing.iter().any(|flag| *flag) {
                snapshot.warnings.push("SOURCE_METRICS_INCOMPLETE".into());
            }
            snapshot.warnings.sort();
            snapshot.warnings.dedup();
            snapshots.push(snapshot);
        }
        let batch = CollectionBatch { snapshots };
        application::validate_batch(&batch)?;
        Ok(batch)
    }
}

fn number(value: &Value, name: &str) -> Result<u64, CoreError> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or(CoreError::SchemaUnsupported)
}
fn normalize_report(kind: AgentKind, bytes: &[u8], report: ReportKind) -> Result<Value, CoreError> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|_| CoreError::SchemaUnsupported)?;
    let key = if report == ReportKind::Daily {
        "daily"
    } else {
        "sessions"
    };
    let rows = value
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or(CoreError::SchemaUnsupported)?;
    for row in rows {
        if report == ReportKind::Session {
            let id = row
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or(CoreError::SchemaUnsupported)?;
            row["sessionId"] = json!(format!(
                "{}-{:x}",
                kind.product(),
                Sha256::digest(id.as_bytes())
            ));
        }
        match kind {
            AgentKind::Codex => {
                row["totalCost"] = row.get("costUSD").cloned().unwrap_or(Value::Null);
                let models = row
                    .get("models")
                    .and_then(Value::as_object)
                    .ok_or(CoreError::SchemaUnsupported)?;
                let names: Vec<_> = models.keys().cloned().collect();
                let breakdowns: Vec<_> = models
                    .iter()
                    .map(|(name, usage)| {
                        let mut usage = usage.clone();
                        usage["modelName"] = json!(name);
                        usage["cost"] = Value::Null;
                        usage
                    })
                    .collect();
                row["modelsUsed"] = json!(names);
                row["modelBreakdowns"] = json!(breakdowns);
            }
            AgentKind::Antigravity => {
                let visible = number(row, "outputTokens")?;
                let total = number(row, "totalTokens")?;
                let input = number(row, "inputTokens")?;
                let read = number(row, "cacheReadTokens")?;
                let write = number(row, "cacheCreationTokens")?;
                let known = input
                    .checked_add(read)
                    .and_then(|n| n.checked_add(write))
                    .and_then(|n| n.checked_add(visible))
                    .ok_or(CoreError::Overflow)?;
                let reasoning = total
                    .checked_sub(known)
                    .ok_or(CoreError::SchemaUnsupported)?;
                row["reasoningOutputTokens"] = json!(reasoning);
                row["outputTokens"] =
                    json!(visible.checked_add(reasoning).ok_or(CoreError::Overflow)?);
                let models = row
                    .get_mut("modelBreakdowns")
                    .and_then(Value::as_array_mut)
                    .ok_or(CoreError::SchemaUnsupported)?;
                let single = models.len() == 1;
                for model in models {
                    if single {
                        if number(model, "inputTokens")? != input
                            || number(model, "cacheReadTokens")? != read
                            || number(model, "cacheCreationTokens")? != write
                            || number(model, "outputTokens")? != visible
                        {
                            return Err(CoreError::SchemaUnsupported);
                        }
                        model["outputTokens"] = json!(visible + reasoning);
                        model["reasoningOutputTokens"] = json!(reasoning);
                        model["totalTokens"] = json!(total);
                    } else {
                        // This release omits per-model extra/reasoning counts; parent total remains authoritative.
                        model["outputTokens"] = Value::Null;
                        model["reasoningOutputTokens"] = Value::Null;
                        model["totalTokens"] = Value::Null;
                    }
                }
            }
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn antigravity_recovers_output_subset_without_double_counting() {
        let report = json!({"daily":[{"date":"2026-10-04","inputTokens":100,"cacheReadTokens":50,"cacheCreationTokens":10,"outputTokens":30,"totalTokens":210,"totalCost":0.01,"modelsUsed":["gemini-3-pro"],"modelBreakdowns":[{"modelName":"gemini-3-pro","inputTokens":100,"cacheReadTokens":50,"cacheCreationTokens":10,"outputTokens":30,"cost":0.01}]}],"totals":{"totalTokens":210}});
        let normalized = normalize_report(
            AgentKind::Antigravity,
            &serde_json::to_vec(&report).unwrap(),
            ReportKind::Daily,
        )
        .unwrap();
        assert_eq!(normalized["daily"][0]["outputTokens"], 50);
        assert_eq!(normalized["daily"][0]["reasoningOutputTokens"], 20);
        let snapshot = decode_report(
            &serde_json::to_vec(&normalized).unwrap(),
            ReportKind::Daily,
            "UTC",
            "dataset",
            "device",
            Utc::now(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .rows
                .iter()
                .find(|row| row.key.model_id.is_none())
                .unwrap()
                .tokens
                .total
                .value,
            Some(210)
        );
    }
    #[test]
    fn codex_report_input_is_already_uncached_and_sessions_do_not_expose_paths() {
        let report = json!({"sessions":[{"sessionId":"/private/USER/session.jsonl","inputTokens":80,"cacheReadTokens":20,"cacheCreationTokens":0,"outputTokens":30,"reasoningOutputTokens":10,"totalTokens":130,"costUSD":0.01,"models":{"gpt-5":{"inputTokens":80,"cacheReadTokens":20,"cacheCreationTokens":0,"outputTokens":30,"reasoningOutputTokens":10,"totalTokens":130}}}],"totals":{"totalTokens":130}});
        let normalized = normalize_report(
            AgentKind::Codex,
            &serde_json::to_vec(&report).unwrap(),
            ReportKind::Session,
        )
        .unwrap();
        assert_eq!(normalized["sessions"][0]["inputTokens"], 80);
        assert!(!normalized["sessions"][0]["sessionId"]
            .as_str()
            .unwrap()
            .contains("private"));
        let snapshot = decode_report(
            &serde_json::to_vec(&normalized).unwrap(),
            ReportKind::Session,
            "UTC",
            "dataset",
            "device",
            Utc::now(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(snapshot.rows[0].tokens.output_reasoning.value, Some(10));
        assert_eq!(snapshot.rows[0].tokens.output_total.value, Some(30));
    }
}
