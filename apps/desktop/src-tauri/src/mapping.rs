use usage_contracts as api;
use usage_core as core;

pub fn error(error: core::CoreError) -> api::ApiError {
    use api::ErrorCode as A;
    use core::CoreError as C;
    let code = match error {
        C::InvalidQuery => A::InvalidQuery,
        C::InvalidData => A::InvalidData,
        C::ProviderNotFound => A::ProviderNotFound,
        C::ProviderDisabled => A::ProviderDisabled,
        C::SourceNotDetected => A::SourceNotDetected,
        C::PermissionDenied => A::PermissionDenied,
        C::SchemaUnsupported => A::SchemaUnsupported,
        C::CoverageIncomplete => A::CoverageIncomplete,
        C::Timeout => A::Timeout,
        C::OutputLimitExceeded => A::OutputLimitExceeded,
        C::Cancelled => A::Cancelled,
        C::Storage => A::Storage,
        C::StorageSchemaNewer => A::StorageSchemaNewer,
        C::UnsupportedFilter => A::UnsupportedFilter,
        C::DatasetConflict => A::DatasetConflict,
        C::Overflow => A::Overflow,
        C::ScanNotFound => A::ScanNotFound,
        C::ScanBusy => A::ScanBusy,
        C::ShuttingDown => A::ShuttingDown,
        C::CollectionFailed => A::CollectionFailed,
    };
    api::ApiError {
        api_version: api::API_VERSION.into(),
        code,
        message: error.to_string(),
        retryable: matches!(
            error,
            C::Timeout | C::Storage | C::ScanBusy | C::CollectionFailed
        ),
    }
}
fn accuracy(value: core::Accuracy) -> api::Accuracy {
    match value {
        core::Accuracy::Exact => api::Accuracy::Exact,
        core::Accuracy::Derived => api::Accuracy::Derived,
        core::Accuracy::Estimated => api::Accuracy::Estimated,
        core::Accuracy::Unavailable => api::Accuracy::Unavailable,
    }
}
fn metric<T: ToString>(value: &core::AggregateMetric<T>) -> api::Metric<String> {
    api::Metric {
        value: value.metric.value.as_ref().map(ToString::to_string),
        accuracy: accuracy(value.metric.accuracy),
        known_rows: value.known_rows,
        missing_rows: value.missing_rows,
    }
}
pub fn aggregate(value: &core::Aggregate) -> api::UsageAggregate {
    api::UsageAggregate {
        tokens: api::TokenMetrics {
            input_uncached: metric(&value.tokens.input_uncached),
            cache_read: metric(&value.tokens.cache_read),
            cache_write: metric(&value.tokens.cache_write),
            output_total: metric(&value.tokens.output_total),
            output_reasoning: metric(&value.tokens.output_reasoning),
            total: metric(&value.tokens.total),
        },
        cost: api::CostEstimate {
            amount_usd: metric(&value.cost.amount_usd),
            kind: api::CostKind::ApiEquivalentEstimate,
            pricing_versions: value.cost.pricing_versions.clone(),
            pricing_as_of: value.cost.pricing_as_of.map(|d| d.to_rfc3339()),
            missing_models: value.cost.missing_models.clone(),
        },
    }
}
fn range(value: &core::DateRange) -> api::DateRange {
    api::DateRange {
        start: value.start.to_string(),
        end: value.end.to_string(),
    }
}
pub fn coverage(value: core::Coverage) -> api::Coverage {
    api::Coverage {
        state: match value.state {
            core::CoverageState::Complete => api::CoverageState::Complete,
            core::CoverageState::Partial => api::CoverageState::Partial,
            core::CoverageState::Archived => api::CoverageState::Archived,
        },
        range: value.range.as_ref().map(range),
        observed_from: value.observed_from.map(|d| d.to_string()),
        observed_until: value.observed_until.map(|d| d.to_string()),
    }
}
pub fn scan(value: core::ScanRecord) -> api::ScanSummary {
    api::ScanSummary {
        api_version: api::API_VERSION.into(),
        job_id: value.job_id,
        provider_id: value.provider_id,
        state: match value.state {
            core::ScanState::Queued => api::ScanState::Queued,
            core::ScanState::Running => api::ScanState::Running,
            core::ScanState::Succeeded => api::ScanState::Succeeded,
            core::ScanState::Failed => api::ScanState::Failed,
            core::ScanState::Cancelled => api::ScanState::Cancelled,
        },
        started_at: value.started_at.to_rfc3339(),
        finished_at: value.finished_at.map(|d| d.to_rfc3339()),
        error: value.error.map(error),
        snapshots_replaced: value.snapshots_replaced,
        rows_written: value.rows_written,
    }
}
pub fn provider(value: core::ProviderStatus) -> api::ProviderSummary {
    let cap = value.descriptor.capabilities;
    api::ProviderSummary {
        provider_id: value.descriptor.provider_id,
        product_id: value.descriptor.product_id,
        display_name: value.descriptor.display_name,
        enabled: value.enabled,
        state: match value.detection.state {
            core::ProviderState::NotDetected => api::ProviderState::NotDetected,
            core::ProviderState::NoData => api::ProviderState::NoData,
            core::ProviderState::Ready => api::ProviderState::Ready,
            core::ProviderState::Scanning => api::ProviderState::Scanning,
            core::ProviderState::PermissionDenied => api::ProviderState::PermissionDenied,
            core::ProviderState::SchemaUnsupported => api::ProviderState::SchemaUnsupported,
            core::ProviderState::Partial => api::ProviderState::Partial,
            core::ProviderState::Stale => api::ProviderState::Stale,
            core::ProviderState::Error => api::ProviderState::Error,
        },
        path_hint: value.detection.path_hint,
        capabilities: api::ProviderCapabilities {
            report_kinds: cap
                .report_kinds
                .into_iter()
                .map(|kind| match kind {
                    core::ReportKind::Daily => api::ReportKind::Daily,
                    core::ReportKind::Session => api::ReportKind::Session,
                })
                .collect(),
            supported_dimensions: cap.supported_dimensions,
            supported_metrics: cap.supported_metrics,
            supports_date_session_intersection: cap.supports_date_session_intersection,
            supports_incremental_collection: cap.supports_incremental_collection,
            supports_quota: cap.supports_quota,
        },
        last_success_at: value.last_success_at.map(|d| d.to_rfc3339()),
        coverage: value.coverage.into_iter().map(coverage).collect(),
        last_scan: value.last_scan.map(scan),
    }
}
fn parse_range(value: &api::DateRange) -> Result<core::DateRange, core::CoreError> {
    let parse = |s: &str| {
        if s.len() != 10 {
            return Err(core::CoreError::InvalidQuery);
        }
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| core::CoreError::InvalidQuery)
    };
    let result = core::DateRange {
        start: parse(&value.start)?,
        end: parse(&value.end)?,
    };
    result.validate()?;
    Ok(result)
}
fn validate_ids(ids: &[String]) -> Result<(), core::CoreError> {
    if ids.len() > 50 || ids.iter().any(|id| id.is_empty() || id.len() > 256) {
        Err(core::CoreError::InvalidQuery)
    } else {
        Ok(())
    }
}
pub fn overview_query(value: &api::OverviewQuery) -> Result<core::OverviewQuery, core::CoreError> {
    validate_ids(&value.provider_ids)?;
    validate_ids(&value.model_ids)?;
    core::application::validate_timezone(&value.timezone)?;
    Ok(core::OverviewQuery {
        range: parse_range(&value.range)?,
        timezone: value.timezone.clone(),
        provider_ids: value.provider_ids.clone(),
        model_ids: value.model_ids.clone(),
        bucket: match value.bucket {
            api::Bucket::Day => core::Bucket::Day,
            api::Bucket::Week => core::Bucket::Week,
            api::Bucket::Month => core::Bucket::Month,
        },
    })
}
pub fn session_query(value: &api::SessionQuery) -> Result<core::SessionQuery, core::CoreError> {
    validate_ids(&value.provider_ids)?;
    validate_ids(&value.model_ids)?;
    core::application::validate_timezone(&value.timezone)?;
    Ok(core::SessionQuery {
        timezone: value.timezone.clone(),
        provider_ids: value.provider_ids.clone(),
        model_ids: value.model_ids.clone(),
        active_range: value.active_range.as_ref().map(parse_range).transpose()?,
        offset: value.offset,
        limit: value.limit,
    })
}
pub fn overview(value: core::Overview, query: api::OverviewQuery) -> api::OverviewResult {
    api::OverviewResult {
        api_version: api::API_VERSION.into(),
        query,
        usage: aggregate(&value.aggregate),
        by_provider: value
            .by_provider
            .into_iter()
            .map(|(id, usage)| api::UsageGroup {
                id,
                usage: aggregate(&usage),
            })
            .collect(),
        by_model: value
            .by_model
            .into_iter()
            .map(|(id, usage)| api::UsageGroup {
                id,
                usage: aggregate(&usage),
            })
            .collect(),
        buckets: value
            .buckets
            .into_iter()
            .map(|(date, usage)| api::TimeBucket {
                start: date.to_string(),
                usage: aggregate(&usage),
            })
            .collect(),
        coverage: value.coverage.into_iter().map(coverage).collect(),
        warnings: value.warnings,
        stale: value.stale,
        last_success_at: value.last_success_at.map(|d| d.to_rfc3339()),
    }
}
pub fn sessions(value: core::SessionPage) -> api::SessionPage {
    let mut items = Vec::new();
    for entry in value.items {
        items.push(api::SessionItem {
            session_id: entry.session_id,
            provider_id: entry.provider_id,
            product_id: entry.product_id,
            source_dataset_id: entry.source_dataset_id,
            origin_device_id: entry.origin_device_id,
            model_id: entry.model_id,
            model_vendor: entry.model_vendor,
            started_at: entry.session_started_at.map(|d| d.to_rfc3339()),
            last_activity_at: entry.last_activity_at.map(|d| d.to_rfc3339()),
            usage: aggregate(&entry.usage),
        });
    }
    api::SessionPage {
        api_version: api::API_VERSION.into(),
        items,
        total: value.total,
        next_offset: value.next_offset,
        stale: value.stale,
        warnings: value.warnings,
        date_filter_semantics: "active-sessions-lifetime-usage".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_preserves_large_partial_token_counts_and_stable_error_codes() {
        let mut usage = core::Aggregate::default();
        usage.tokens.total = core::AggregateMetric {
            metric: core::Metric::exact(i64::MAX as u64),
            known_rows: 1,
            missing_rows: 2,
        };
        let dto = aggregate(&usage);
        assert_eq!(
            dto.tokens.total.value.as_deref(),
            Some("9223372036854775807")
        );
        assert_eq!(dto.tokens.total.missing_rows, 2);
        assert!(dto.tokens.cache_read.value.is_none());
        assert_eq!(
            error(core::CoreError::PermissionDenied).code,
            api::ErrorCode::PermissionDenied
        );
        assert_eq!(dto.cost.kind, api::CostKind::ApiEquivalentEstimate);
    }
    #[test]
    fn query_mapping_rejects_bad_ranges_and_timezone() {
        let query = api::OverviewQuery {
            range: api::DateRange {
                start: "2026-10-04".into(),
                end: "2026-10-04".into(),
            },
            timezone: "America/Phoenix".into(),
            provider_ids: vec![],
            model_ids: vec![],
            bucket: api::Bucket::Day,
        };
        assert_eq!(
            overview_query(&query).unwrap_err(),
            core::CoreError::InvalidQuery
        );
    }
}
