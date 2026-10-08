use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "1.3.0";
pub const SCAN_EVENT: &str = "usage://scan-updated";
pub const AUTO_COLLECTION_EVENT: &str = "usage://auto-collection-updated";
pub const TIMEZONE_EVENT: &str = "usage://timezone-updated";
pub const COMMANDS: &[(&str, &str)] = &[
    ("getApiInfo", "get_api_info"),
    ("listProviders", "list_providers"),
    ("startScan", "start_scan"),
    ("getScan", "get_scan"),
    ("cancelScan", "cancel_scan"),
    ("getOverview", "get_overview"),
    ("listSessions", "list_sessions"),
    ("exportUsage", "export_usage"),
    ("getSettings", "get_settings"),
    ("updateSettings", "update_settings"),
    ("chooseProviderDirectory", "choose_provider_directory"),
    ("getAutoCollection", "get_auto_collection"),
    ("updateAutoCollection", "update_auto_collection"),
    ("getTimezone", "get_timezone"),
    ("updateTimezone", "update_timezone"),
];

macro_rules! dto {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}
macro_rules! enumeration {
    ($name:ident, $rename:literal, $($variant:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = $rename)]
        pub enum $name { $($variant),+ }
    };
}
enumeration!(
    Accuracy,
    "kebab-case",
    Exact,
    Derived,
    Estimated,
    Unavailable
);
enumeration!(ReportKind, "kebab-case", Daily, Session);
enumeration!(CoverageState, "kebab-case", Complete, Partial, Archived);
enumeration!(
    ProviderState,
    "kebab-case",
    NotDetected,
    NoData,
    Ready,
    Scanning,
    PermissionDenied,
    SchemaUnsupported,
    Partial,
    Stale,
    Error
);
enumeration!(
    ScanState,
    "kebab-case",
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled
);
enumeration!(Bucket, "kebab-case", Day, Week, Month);
enumeration!(ExportFormat, "kebab-case", Json, Csv);
enumeration!(CostKind, "kebab-case", ApiEquivalentEstimate);
enumeration!(
    ErrorCode,
    "SCREAMING_SNAKE_CASE",
    InvalidQuery,
    InvalidData,
    ProviderNotFound,
    ProviderDisabled,
    SourceNotDetected,
    PermissionDenied,
    SchemaUnsupported,
    CoverageIncomplete,
    Timeout,
    OutputLimitExceeded,
    Cancelled,
    Storage,
    StorageSchemaNewer,
    UnsupportedFilter,
    DatasetConflict,
    Overflow,
    ScanNotFound,
    ScanBusy,
    ShuttingDown,
    CollectionFailed,
    ApiVersionUnsupported,
    ExportFailed,
    Internal,
    SettingsConflict,
    InvalidDirectoryRef
);

/// Decimal strings for all token counts; USD also uses decimal strings. value=null iff accuracy=unavailable.
/// Aggregates expose knownRows/missingRows, so a known partial sum cannot masquerade as complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metric<T> {
    pub value: Option<T>,
    pub accuracy: Accuracy,
    pub known_rows: u32,
    pub missing_rows: u32,
}
dto!(TokenMetrics { input_uncached: Metric<String>, cache_read: Metric<String>, cache_write: Metric<String>, output_total: Metric<String>, output_reasoning: Metric<String>, total: Metric<String> });
dto!(CostEstimate { amount_usd: Metric<String>, kind: CostKind, pricing_versions: Vec<String>, pricing_as_of: Option<String>, missing_models: Vec<String> });
dto!(UsageAggregate {
    tokens: TokenMetrics,
    cost: CostEstimate
});
dto!(DateRange {
    start: String,
    end: String
});
dto!(Coverage { state: CoverageState, range: Option<DateRange>, observed_from: Option<String>, observed_until: Option<String> });
dto!(ProviderCapabilities { report_kinds: Vec<ReportKind>, supported_dimensions: Vec<String>, supported_metrics: Vec<String>, supports_date_session_intersection: bool, supports_incremental_collection: bool, supports_quota: bool });
dto!(ApiInfo { api_version: String, app_version: String, capabilities: Vec<String> });
dto!(ApiError {
    api_version: String,
    code: ErrorCode,
    message: String,
    retryable: bool
});
dto!(ProviderSummary { provider_id: String, product_id: String, display_name: String, enabled: bool, state: ProviderState, path_hint: Option<String>, capabilities: ProviderCapabilities, last_success_at: Option<String>, coverage: Vec<Coverage>, last_scan: Option<ScanSummary> });
dto!(StartScanRequest {
    provider_id: String,
    timezone: String
});
dto!(StartScanResult { job_id: String });
dto!(JobRequest { job_id: String });
dto!(ScanSummary { api_version: String, job_id: String, provider_id: String, state: ScanState, started_at: String, finished_at: Option<String>, error: Option<ApiError>, snapshots_replaced: u32, rows_written: u32 });
dto!(OverviewQuery { range: DateRange, timezone: String, provider_ids: Vec<String>, model_ids: Vec<String>, bucket: Bucket });
dto!(UsageGroup {
    id: String,
    usage: UsageAggregate
});
dto!(TimeBucket {
    start: String,
    usage: UsageAggregate
});
dto!(OverviewResult { api_version: String, query: OverviewQuery, usage: UsageAggregate, by_provider: Vec<UsageGroup>, by_model: Vec<UsageGroup>, buckets: Vec<TimeBucket>, coverage: Vec<Coverage>, warnings: Vec<String>, stale: bool, last_success_at: Option<String> });
dto!(SessionQuery { timezone: String, provider_ids: Vec<String>, model_ids: Vec<String>, active_range: Option<DateRange>, offset: u32, limit: u32 });
dto!(SessionItem { session_id: String, provider_id: String, product_id: String, source_dataset_id: String, origin_device_id: String, model_id: Option<String>, model_vendor: Option<String>, started_at: Option<String>, last_activity_at: Option<String>, usage: UsageAggregate });
dto!(SessionPage { api_version: String, items: Vec<SessionItem>, total: u32, next_offset: Option<u32>, stale: bool, warnings: Vec<String>, date_filter_semantics: String });
dto!(ExportRequest {
    format: ExportFormat,
    query: OverviewQuery
});
dto!(ExportResult {
    api_version: String,
    export_id: String,
    suggested_filename: String,
    media_type: String,
    byte_length: String
});

dto!(ArchiveScope { kind: String, range: Option<DateRange>, model_ids: Vec<String> });
dto!(ArchiveRow { date: Option<String>, session_id: Option<String>, model_id: Option<String>, model_vendor: Option<String>, usage: UsageAggregate, started_at: Option<String>, last_activity_at: Option<String> });
dto!(ArchiveSnapshot { provider_id: String, product_id: String, source_dataset_id: String, origin_device_id: String, report_kind: ReportKind, timezone: String, scope: ArchiveScope, revision: String, collected_at: String, collection_started_at: String, collector_version: String, normalization_version: String, coverage: Coverage, warnings: Vec<String>, rows: Vec<ArchiveRow> });
dto!(UsageArchive { archive_schema_version: String, app_version: String, exported_at: String, snapshots: Vec<ArchiveSnapshot> });

dto!(SourceDirectory {
    directory_ref: String,
    label: String
});
dto!(ProviderSettings { provider_id: String, enabled: bool, directory: Option<SourceDirectory> });
dto!(SettingsResult { api_version: String, revision: String, timezone: String, providers: Vec<ProviderSettings>, collection_notice: String, directory_change_policy: String });
dto!(ProviderSettingsUpdate { provider_id: String, enabled: bool, directory_ref: Option<String> });
dto!(UpdateSettingsRequest { expected_revision: String, timezone: String, providers: Vec<ProviderSettingsUpdate> });
dto!(ChooseProviderDirectoryRequest {
    provider_id: String
});
dto!(ChooseProviderDirectoryResult { api_version: String, provider_id: String, directory: Option<SourceDirectory> });

enumeration!(
    AutoCollectionState,
    "kebab-case",
    Disabled,
    Idle,
    Checking,
    Scanning,
    Waiting,
    Backoff
);
dto!(AutoCollectionConfig {
    enabled: bool,
    interval_minutes: u32
});
dto!(UpdateAutoCollectionRequest {
    expected_revision: String,
    config: AutoCollectionConfig
});
dto!(AutoProviderStatus { provider_id: String, state: AutoCollectionState, job_id: Option<String>, last_success_at: Option<String>, next_check_at: Option<String>, watching: bool, error: Option<ApiError> });
dto!(AutoCollectionStatus { api_version: String, revision: String, config: AutoCollectionConfig, timezone: String, providers: Vec<AutoProviderStatus> });

enumeration!(TimezoneMode, "kebab-case", FollowSystem, Fixed);
enumeration!(
    TimezoneRebuildState,
    "kebab-case",
    Idle,
    Pending,
    Rebuilding,
    Backoff
);
enumeration!(
    TimezoneProviderState,
    "kebab-case",
    Pending,
    Rebuilding,
    Succeeded,
    Failed
);
// Fixed requires a valid IANA timezone; FollowSystem requires null and resolves in the host.
dto!(UpdateTimezoneRequest { expected_revision: String, mode: TimezoneMode, timezone: Option<String> });
dto!(TimezoneProviderStatus { provider_id: String, state: TimezoneProviderState, job_id: Option<String>, error: Option<ApiError> });
// sequence is a decimal counter; clients ignore a status older than one already applied.
dto!(TimezoneStatus { api_version: String, sequence: String, revision: String, mode: TimezoneMode, effective_timezone: String, system_timezone: Option<String>, detection_error: Option<ApiError>, pending_timezone: Option<String>, rebuild: TimezoneRebuildState, next_retry_at: Option<String>, providers: Vec<TimezoneProviderStatus> });
