use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Accuracy {
    Exact,
    Derived,
    Estimated,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metric<T> {
    pub value: Option<T>,
    pub accuracy: Accuracy,
}
impl<T> Metric<T> {
    pub fn exact(value: T) -> Self {
        Self {
            value: Some(value),
            accuracy: Accuracy::Exact,
        }
    }
    pub fn unavailable() -> Self {
        Self {
            value: None,
            accuracy: Accuracy::Unavailable,
        }
    }
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.value.is_none() != (self.accuracy == Accuracy::Unavailable) {
            return Err(CoreError::InvalidData);
        }
        Ok(())
    }
}
impl<T> Default for Metric<T> {
    fn default() -> Self {
        Self::unavailable()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMetrics {
    pub input_uncached: Metric<u64>,
    pub cache_read: Metric<u64>,
    pub cache_write: Metric<u64>,
    pub output_total: Metric<u64>,
    pub output_reasoning: Metric<u64>,
    pub total: Metric<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostEstimate {
    pub amount_usd: Metric<Decimal>,
    pub pricing_version: Option<String>,
    pub pricing_as_of: Option<DateTime<Utc>>,
    pub missing_models: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportKind {
    Daily,
    Session,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}
impl DateRange {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.start >= self.end {
            Err(CoreError::InvalidQuery)
        } else {
            Ok(())
        }
    }
    pub fn contains(&self, date: NaiveDate) -> bool {
        date >= self.start && date < self.end
    }
}
/// Standard is the only authoritative view. Filtered caches never contribute to global totals.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QueryScope {
    Standard,
    Filtered {
        start: String,
        end: String,
        model_ids: Vec<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SnapshotKey {
    pub product_id: String,
    pub source_dataset_id: String,
    pub report_kind: ReportKind,
    pub timezone: String,
    pub scope: QueryScope,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageState {
    Complete,
    Partial,
    Archived,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub state: CoverageState,
    pub range: Option<DateRange>,
    pub observed_from: Option<NaiveDate>,
    pub observed_until: Option<NaiveDate>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RowDimension {
    Day(NaiveDate),
    Session(String),
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RowKey {
    pub dimension: RowDimension,
    pub model_id: Option<String>,
}
/// model_id=None is a parent total; model detail must never be added to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRow {
    pub key: RowKey,
    pub model_vendor: Option<String>,
    pub tokens: TokenMetrics,
    pub cost: CostEstimate,
    pub session_started_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSnapshot {
    pub key: SnapshotKey,
    pub provider_id: String,
    pub origin_device_id: String,
    pub revision: u64,
    pub collected_at: DateTime<Utc>,
    pub collection_started_at: DateTime<Utc>,
    pub collector_version: String,
    pub normalization_version: String,
    pub coverage: Coverage,
    pub warnings: Vec<String>,
    pub rows: Vec<ReportRow>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionBatch {
    pub snapshots: Vec<ReportSnapshot>,
}
#[derive(Debug, Clone, Default)]
pub struct SnapshotFilter {
    pub provider_id: Option<String>,
    pub report_kind: Option<ReportKind>,
    pub timezone: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitResult {
    pub snapshots_replaced: u32,
    pub rows_written: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub report_kinds: Vec<ReportKind>,
    pub supported_dimensions: Vec<String>,
    pub supported_metrics: Vec<String>,
    pub supports_date_session_intersection: bool,
    pub supports_incremental_collection: bool,
    pub supports_quota: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub provider_id: String,
    pub product_id: String,
    pub display_name: String,
    pub capabilities: ProviderCapabilities,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceConfig {
    pub enabled: bool,
    pub root_path: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderState {
    NotDetected,
    NoData,
    Ready,
    Scanning,
    PermissionDenied,
    SchemaUnsupported,
    Partial,
    Stale,
    Error,
}
#[derive(Debug, Clone)]
pub struct Detection {
    pub state: ProviderState,
    pub path_hint: Option<String>,
}
#[derive(Debug, Clone)]
pub struct CollectRequest {
    pub timezone: String,
    pub config: SourceConfig,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
impl ScanState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRecord {
    pub job_id: String,
    pub provider_id: String,
    pub state: ScanState,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<CoreError>,
    pub snapshots_replaced: u32,
    pub rows_written: u32,
}
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    pub descriptor: ProviderDescriptor,
    pub enabled: bool,
    pub detection: Detection,
    pub last_scan: Option<ScanRecord>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub coverage: Vec<Coverage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoreError {
    #[error("Invalid query")]
    InvalidQuery,
    #[error("Invalid normalized data")]
    InvalidData,
    #[error("Provider not found")]
    ProviderNotFound,
    #[error("Provider disabled")]
    ProviderDisabled,
    #[error("Source not detected")]
    SourceNotDetected,
    #[error("Source permission denied")]
    PermissionDenied,
    #[error("Source schema unsupported")]
    SchemaUnsupported,
    #[error("Coverage incomplete; previous snapshot retained")]
    CoverageIncomplete,
    #[error("Collection timed out")]
    Timeout,
    #[error("Output limit exceeded")]
    OutputLimitExceeded,
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Storage unavailable")]
    Storage,
    #[error("Stored schema is newer than supported")]
    StorageSchemaNewer,
    #[error("Unsupported filter combination")]
    UnsupportedFilter,
    #[error("Conflicting dataset ownership")]
    DatasetConflict,
    #[error("Metric overflow")]
    Overflow,
    #[error("Scan not found")]
    ScanNotFound,
    #[error("Scan already running")]
    ScanBusy,
    #[error("Application is shutting down")]
    ShuttingDown,
    #[error("Collection failed")]
    CollectionFailed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Day,
    Week,
    Month,
}
#[derive(Debug, Clone)]
pub struct OverviewQuery {
    pub range: DateRange,
    pub timezone: String,
    pub provider_ids: Vec<String>,
    pub model_ids: Vec<String>,
    pub bucket: Bucket,
}
#[derive(Debug, Clone)]
pub struct SessionQuery {
    pub timezone: String,
    pub provider_ids: Vec<String>,
    pub model_ids: Vec<String>,
    pub active_range: Option<DateRange>,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateMetric<T> {
    pub metric: Metric<T>,
    pub known_rows: u32,
    pub missing_rows: u32,
}
impl<T> Default for AggregateMetric<T> {
    fn default() -> Self {
        Self {
            metric: Metric::unavailable(),
            known_rows: 0,
            missing_rows: 0,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AggregateTokens {
    pub input_uncached: AggregateMetric<u64>,
    pub cache_read: AggregateMetric<u64>,
    pub cache_write: AggregateMetric<u64>,
    pub output_total: AggregateMetric<u64>,
    pub output_reasoning: AggregateMetric<u64>,
    pub total: AggregateMetric<u64>,
}
#[derive(Debug, Clone, Default)]
pub struct AggregateCost {
    pub amount_usd: AggregateMetric<Decimal>,
    pub missing_models: Vec<String>,
    pub pricing_versions: Vec<String>,
    pub pricing_as_of: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Default)]
pub struct Aggregate {
    pub tokens: AggregateTokens,
    pub cost: AggregateCost,
}
#[derive(Debug, Clone)]
pub struct Overview {
    pub aggregate: Aggregate,
    pub by_provider: BTreeMap<String, Aggregate>,
    pub by_model: BTreeMap<String, Aggregate>,
    pub buckets: BTreeMap<NaiveDate, Aggregate>,
    pub coverage: Vec<Coverage>,
    pub warnings: Vec<String>,
    pub stale: bool,
    pub last_success_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub provider_id: String,
    pub product_id: String,
    pub source_dataset_id: String,
    pub origin_device_id: String,
    pub session_id: String,
    pub model_id: Option<String>,
    pub model_vendor: Option<String>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub usage: Aggregate,
}
#[derive(Debug, Clone)]
pub struct SessionPage {
    pub items: Vec<SessionEntry>,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub stale: bool,
    pub warnings: Vec<String>,
}
