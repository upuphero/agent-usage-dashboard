use crate::*;
use chrono::{Datelike, Duration, NaiveDate};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
pub struct UsageService {
    pub repository: Arc<dyn UsageRepository>,
    pub clock: Arc<dyn Clock>,
    pub sources: Vec<Arc<dyn UsageSource>>,
    observed_scans: Mutex<BTreeMap<String, ScanRecord>>,
}
impl UsageService {
    pub fn new(
        repository: Arc<dyn UsageRepository>,
        clock: Arc<dyn Clock>,
        sources: Vec<Arc<dyn UsageSource>>,
    ) -> Self {
        Self {
            repository,
            clock,
            sources,
            observed_scans: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn source(&self, provider: &str) -> Result<&Arc<dyn UsageSource>, CoreError> {
        self.sources
            .iter()
            .find(|s| s.descriptor().provider_id == provider)
            .ok_or(CoreError::ProviderNotFound)
    }

    pub async fn list_providers(
        &self,
        configs: &BTreeMap<String, SourceConfig>,
    ) -> Result<Vec<ProviderStatus>, CoreError> {
        let mut result = Vec::new();
        for source in &self.sources {
            let descriptor = source.descriptor();
            let config = configs
                .get(&descriptor.provider_id)
                .cloned()
                .unwrap_or_default();
            let mut detection = match source.detect(&config).await {
                Ok(detection) => detection,
                Err(error) => Detection {
                    state: provider_error_state(error),
                    path_hint: None,
                },
            };
            let snapshots = self
                .repository
                .load_snapshots(SnapshotFilter {
                    provider_id: Some(descriptor.provider_id.clone()),
                    ..Default::default()
                })
                .await?;
            let scans = self.repository.list_scans(&descriptor.provider_id).await?;
            let last_scan = self.latest_scan(&descriptor.provider_id, scans)?;
            let last_success_at = snapshots.iter().map(|s| s.collected_at).max();
            if detection.state == ProviderState::Ready
                && !snapshots.is_empty()
                && snapshots.iter().all(|s| s.rows.is_empty())
            {
                detection.state = ProviderState::NoData;
            }
            let mut status = ProviderStatus {
                descriptor,
                enabled: config.enabled,
                detection,
                last_scan: None,
                last_success_at,
                coverage: snapshots.into_iter().map(|s| s.coverage).collect(),
            };
            apply_provider_scan(&mut status, last_scan);
            result.push(status);
        }
        Ok(result)
    }

    /// The host owns job scheduling; this use case owns collection validation and atomic publication.
    pub async fn run_scan(
        &self,
        job_id: String,
        provider_id: String,
        request: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<ScanRecord, CoreError> {
        self.source(&provider_id)?;
        let started_at = match self.repository.get_scan(&job_id).await? {
            Some(scan) if scan.provider_id == provider_id && scan.state == ScanState::Queued => {
                scan.started_at
            }
            Some(_) => return Err(CoreError::InvalidQuery),
            None => self.clock.now(),
        };
        let mut scan = ScanRecord {
            job_id,
            provider_id: provider_id.clone(),
            state: ScanState::Running,
            started_at,
            finished_at: None,
            error: None,
            snapshots_replaced: 0,
            rows_written: 0,
        };
        self.repository.save_scan(scan.clone()).await?;
        self.observe_scan(scan.clone())?;
        let outcome = async {
            validate_timezone(&request.timezone)?;
            cancellation.check()?;
            if !request.config.enabled {
                return Err(CoreError::ProviderDisabled);
            }
            let source = self.source(&provider_id)?;
            let descriptor = source.descriptor();
            let detection = source.detect(&request.config).await?;
            match detection.state {
                ProviderState::NotDetected => return Err(CoreError::SourceNotDetected),
                ProviderState::PermissionDenied => return Err(CoreError::PermissionDenied),
                ProviderState::SchemaUnsupported => return Err(CoreError::SchemaUnsupported),
                ProviderState::Error => return Err(CoreError::CollectionFailed),
                _ => (),
            }
            let timezone = request.timezone.clone();
            let mut batch = source.collect(request, cancellation.clone()).await?;
            cancellation.check()?;
            normalize_batch(&mut batch)?;
            validate_batch(&batch)?;
            if batch.snapshots.iter().any(|s| {
                s.provider_id != provider_id
                    || s.key.product_id != descriptor.product_id
                    || s.key.timezone != timezone
                    || !descriptor
                        .capabilities
                        .report_kinds
                        .contains(&s.key.report_kind)
            }) {
                return Err(CoreError::InvalidData);
            }
            // A readable-but-emptied/rotated log directory is not proof that retained history was zero.
            // Without an explicit rebuild operation, missing historical day/session partitions stay retained.
            let previous = self
                .repository
                .load_snapshots(SnapshotFilter {
                    provider_id: Some(provider_id.clone()),
                    ..Default::default()
                })
                .await?;
            for snapshot in batch
                .snapshots
                .iter()
                .filter(|s| s.key.scope == QueryScope::Standard)
            {
                if let Some(old) = previous.iter().find(|old| old.key == snapshot.key) {
                    let current_dimensions: BTreeSet<_> =
                        snapshot.rows.iter().map(|row| &row.key.dimension).collect();
                    if old
                        .rows
                        .iter()
                        .any(|row| !current_dimensions.contains(&row.key.dimension))
                    {
                        return Err(CoreError::CoverageIncomplete);
                    }
                }
            }
            // After this point a committed scan is succeeded even if cancellation arrives during commit.
            cancellation.begin_commit()?;
            self.repository.commit_batch(batch).await
        }
        .await;
        scan.finished_at = Some(self.clock.now());
        match outcome {
            Ok(commit) => {
                scan.state = ScanState::Succeeded;
                scan.snapshots_replaced = commit.snapshots_replaced;
                scan.rows_written = commit.rows_written;
            }
            Err(error) => {
                scan.state = if error == CoreError::Cancelled {
                    ScanState::Cancelled
                } else {
                    ScanState::Failed
                };
                scan.error = Some(error);
            }
        }
        self.observe_scan(scan.clone())?;
        self.repository.save_scan(scan.clone()).await?;
        Ok(scan)
    }

    pub async fn get_overview(&self, query: OverviewQuery) -> Result<Overview, CoreError> {
        query.range.validate()?;
        if (query.range.end - query.range.start).num_days() > 10_000 {
            return Err(CoreError::InvalidQuery);
        }
        validate_timezone(&query.timezone)?;
        self.validate_providers(&query.provider_ids)?;
        let snapshots = self
            .repository
            .load_snapshots(SnapshotFilter {
                report_kind: Some(ReportKind::Daily),
                timezone: Some(query.timezone.clone()),
                ..Default::default()
            })
            .await?;
        let mut result = Overview {
            aggregate: Aggregate::default(),
            by_provider: BTreeMap::new(),
            by_model: BTreeMap::new(),
            buckets: BTreeMap::new(),
            coverage: Vec::new(),
            warnings: Vec::new(),
            stale: false,
            last_success_at: None,
        };
        let mut seen = BTreeSet::new();
        let mut zero_coverage = Vec::new();
        for snapshot in snapshots.into_iter().filter(|s| {
            s.key.scope == QueryScope::Standard && selected(&query.provider_ids, &s.provider_id)
        }) {
            if !seen.insert(snapshot.key.clone()) {
                return Err(CoreError::DatasetConflict);
            }
            result.stale |= self.provider_stale(&snapshot.provider_id).await?
                || snapshot.coverage.state != CoverageState::Complete;
            result.last_success_at = result.last_success_at.max(Some(snapshot.collected_at));
            result.coverage.push(snapshot.coverage.clone());
            zero_coverage.push((
                snapshot.coverage.clone(),
                self.source(&snapshot.provider_id)?
                    .descriptor()
                    .capabilities,
            ));
            result.warnings.extend(snapshot.warnings.clone());
            let mut days: BTreeMap<NaiveDate, Vec<&ReportRow>> = BTreeMap::new();
            for row in &snapshot.rows {
                if let RowDimension::Day(date) = row.key.dimension {
                    if query.range.contains(date) {
                        days.entry(date).or_default().push(row);
                    }
                }
            }
            if days.is_empty() && snapshot.coverage.state == CoverageState::Complete {
                let capabilities = self
                    .source(&snapshot.provider_id)?
                    .descriptor()
                    .capabilities;
                add_empty_coverage(&mut result.aggregate, &capabilities);
                add_empty_coverage(
                    result
                        .by_provider
                        .entry(snapshot.provider_id.clone())
                        .or_default(),
                    &capabilities,
                );
            }
            for (date, rows) in days {
                let details: Vec<_> = rows
                    .iter()
                    .copied()
                    .filter(|r| r.key.model_id.is_some())
                    .collect();
                let parents: Vec<_> = rows
                    .iter()
                    .copied()
                    .filter(|r| r.key.model_id.is_none())
                    .collect();
                if !query.model_ids.is_empty() && details.is_empty() {
                    return Err(CoreError::UnsupportedFilter);
                }
                let chosen: Vec<_> = if query.model_ids.is_empty() {
                    if parents.is_empty() {
                        details.clone()
                    } else {
                        parents
                    }
                } else {
                    details
                        .iter()
                        .copied()
                        .filter(|r| selected(&query.model_ids, r.key.model_id.as_deref().unwrap()))
                        .collect()
                };
                if details.is_empty() {
                    result.warnings.push("MODEL_BREAKDOWN_UNAVAILABLE".into());
                }
                for row in chosen {
                    add_row(&mut result.aggregate, row)?;
                    add_row(
                        result
                            .by_provider
                            .entry(snapshot.provider_id.clone())
                            .or_default(),
                        row,
                    )?;
                    add_row(
                        result
                            .buckets
                            .entry(bucket_start(date, query.bucket)?)
                            .or_default(),
                        row,
                    )?;
                }
                for row in details
                    .into_iter()
                    .filter(|r| selected(&query.model_ids, r.key.model_id.as_deref().unwrap()))
                {
                    add_row(
                        result
                            .by_model
                            .entry(row.key.model_id.clone().unwrap())
                            .or_default(),
                        row,
                    )?;
                }
            }
        }
        // A missing bucket in complete local Daily coverage proves no usage.
        // Explicitly unavailable rows and an uncollected cache never become zero.
        if !zero_coverage.is_empty() {
            let mut date = query.range.start;
            while date < query.range.end {
                let start = bucket_start(date, query.bucket)?;
                let end = match query.bucket {
                    Bucket::Day => start.succ_opt(),
                    Bucket::Week => start.checked_add_signed(Duration::days(7)),
                    Bucket::Month => {
                        if start.month() == 12 {
                            NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)
                        } else {
                            NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)
                        }
                    }
                }
                .ok_or(CoreError::InvalidQuery)?;
                let covered_start = start.max(query.range.start);
                let covered_end = end.min(query.range.end);
                let proves_empty = zero_coverage.iter().all(|(coverage, _)| {
                    coverage.state == CoverageState::Complete
                        && coverage.range.as_ref().is_none_or(|range| {
                            range.start <= covered_start && range.end >= covered_end
                        })
                });
                if !result.buckets.contains_key(&start) && proves_empty {
                    let mut empty = Aggregate::default();
                    for (_, capabilities) in &zero_coverage {
                        add_empty_coverage(&mut empty, capabilities);
                    }
                    result.buckets.insert(start, empty);
                }
                date = end;
            }
        }
        // Empty cache is unavailable; complete authoritative snapshots can prove zero usage.
        result.warnings.sort();
        result.warnings.dedup();
        Ok(result)
    }

    pub async fn list_sessions(&self, query: SessionQuery) -> Result<SessionPage, CoreError> {
        let timezone = validate_timezone(&query.timezone)?;
        self.validate_providers(&query.provider_ids)?;
        if query.limit == 0 || query.limit > 200 {
            return Err(CoreError::InvalidQuery);
        }
        if let Some(range) = &query.active_range {
            range.validate()?;
        }
        let snapshots = self
            .repository
            .load_snapshots(SnapshotFilter {
                report_kind: Some(ReportKind::Session),
                timezone: Some(query.timezone),
                ..Default::default()
            })
            .await?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut stale = false;
        let mut seen = BTreeSet::new();
        for snapshot in snapshots.into_iter().filter(|s| {
            s.key.scope == QueryScope::Standard && selected(&query.provider_ids, &s.provider_id)
        }) {
            if !seen.insert(snapshot.key.clone()) {
                return Err(CoreError::DatasetConflict);
            }
            stale |= self.provider_stale(&snapshot.provider_id).await?
                || snapshot.coverage.state != CoverageState::Complete;
            let mut sessions: BTreeMap<String, Vec<ReportRow>> = BTreeMap::new();
            for row in snapshot.rows {
                if let RowDimension::Session(id) = &row.key.dimension {
                    sessions.entry(id.clone()).or_default().push(row);
                }
            }
            for (session_id, rows) in sessions {
                let last_activity_at = rows.iter().filter_map(|row| row.last_activity_at).max();
                let session_started_at = rows.iter().filter_map(|row| row.session_started_at).min();
                if let Some(range) = &query.active_range {
                    let Some(activity) = last_activity_at else {
                        warnings.push("SESSION_ACTIVITY_UNAVAILABLE".into());
                        continue;
                    };
                    if !range.contains(activity.with_timezone(&timezone).date_naive()) {
                        continue;
                    }
                }
                let parent = rows.iter().find(|row| row.key.model_id.is_none());
                let details: Vec<_> = rows
                    .iter()
                    .filter(|row| row.key.model_id.is_some())
                    .collect();
                if !query.model_ids.is_empty() && details.is_empty() {
                    return Err(CoreError::UnsupportedFilter);
                }
                let chosen: Vec<_> = if query.model_ids.is_empty() {
                    parent.map(|row| vec![row]).unwrap_or(details)
                } else {
                    details
                        .into_iter()
                        .filter(|row| {
                            selected(&query.model_ids, row.key.model_id.as_deref().unwrap())
                        })
                        .collect()
                };
                if chosen.is_empty() {
                    continue;
                }
                let mut usage = Aggregate::default();
                for row in &chosen {
                    add_row(&mut usage, row)?;
                }
                let (model_id, model_vendor) = if chosen.len() == 1 {
                    (
                        chosen[0].key.model_id.clone(),
                        chosen[0].model_vendor.clone(),
                    )
                } else {
                    (None, None)
                };
                items.push(SessionEntry {
                    provider_id: snapshot.provider_id.clone(),
                    product_id: snapshot.key.product_id.clone(),
                    source_dataset_id: snapshot.key.source_dataset_id.clone(),
                    origin_device_id: snapshot.origin_device_id.clone(),
                    session_id,
                    model_id,
                    model_vendor,
                    session_started_at,
                    last_activity_at,
                    usage,
                });
            }
        }
        items.sort_by(|a, b| {
            b.last_activity_at
                .cmp(&a.last_activity_at)
                .then(a.source_dataset_id.cmp(&b.source_dataset_id))
                .then(a.provider_id.cmp(&b.provider_id))
                .then(a.session_id.cmp(&b.session_id))
        });
        let total = u32::try_from(items.len()).map_err(|_| CoreError::Overflow)?;
        let end = query
            .offset
            .checked_add(query.limit)
            .ok_or(CoreError::Overflow)?;
        let next_offset = (end < total).then_some(end);
        warnings.sort();
        warnings.dedup();
        Ok(SessionPage {
            items: items
                .into_iter()
                .skip(query.offset as usize)
                .take(query.limit as usize)
                .collect(),
            total,
            next_offset,
            stale,
            warnings,
        })
    }

    fn validate_providers(&self, ids: &[String]) -> Result<(), CoreError> {
        for id in ids {
            self.source(id)?;
        }
        Ok(())
    }
    async fn provider_stale(&self, id: &str) -> Result<bool, CoreError> {
        let scans = self.repository.list_scans(id).await?;
        Ok(self
            .latest_scan(id, scans)?
            .is_some_and(|s| s.state == ScanState::Failed || s.state == ScanState::Cancelled))
    }
    /// Host recovery/exception handling uses the same per-service observation order as normal scans.
    /// Opaque job IDs and a wall clock are not causal sequence numbers.
    pub fn observe_scan(&self, scan: ScanRecord) -> Result<(), CoreError> {
        self.source(&scan.provider_id)?;
        self.observed_scans
            .lock()
            .map_err(|_| CoreError::Storage)?
            .insert(scan.provider_id.clone(), scan);
        Ok(())
    }
    fn latest_scan(
        &self,
        provider: &str,
        scans: Vec<ScanRecord>,
    ) -> Result<Option<ScanRecord>, CoreError> {
        if let Some(scan) = self
            .observed_scans
            .lock()
            .map_err(|_| CoreError::Storage)?
            .get(provider)
            .cloned()
        {
            return Ok(Some(scan));
        }
        // Cold-start equal timestamps cannot prove ordering; choose a conservative terminal error.
        fn severity(state: ScanState) -> u8 {
            match state {
                ScanState::Succeeded => 0,
                ScanState::Cancelled => 1,
                ScanState::Failed => 2,
                ScanState::Queued | ScanState::Running => 3,
            }
        }
        Ok(scans.into_iter().max_by(|a, b| {
            a.started_at
                .cmp(&b.started_at)
                .then(a.finished_at.cmp(&b.finished_at))
                .then(severity(a.state).cmp(&severity(b.state)))
                .then(a.job_id.cmp(&b.job_id))
        }))
    }
}

/// Shared status rule used by Core and a host's in-memory terminal-state fallback.
pub fn apply_provider_scan(status: &mut ProviderStatus, scan: Option<ScanRecord>) {
    if let Some(scan) = &scan {
        if !scan.state.is_terminal() {
            status.detection.state = ProviderState::Scanning;
        } else if matches!(scan.state, ScanState::Failed | ScanState::Cancelled)
            && status.last_success_at.is_some()
        {
            status.detection.state = ProviderState::Stale;
        } else if scan.state == ScanState::Failed {
            status.detection.state =
                provider_error_state(scan.error.unwrap_or(CoreError::CollectionFailed));
        }
    }
    status.last_scan = scan;
}
fn provider_error_state(error: CoreError) -> ProviderState {
    match error {
        CoreError::PermissionDenied => ProviderState::PermissionDenied,
        CoreError::SchemaUnsupported => ProviderState::SchemaUnsupported,
        CoreError::SourceNotDetected => ProviderState::NotDetected,
        _ => ProviderState::Error,
    }
}
fn selected(ids: &[String], id: &str) -> bool {
    ids.is_empty() || ids.iter().any(|i| i == id)
}
pub fn validate_timezone(timezone: &str) -> Result<Tz, CoreError> {
    timezone.parse().map_err(|_| CoreError::InvalidQuery)
}
pub fn bucket_start(date: NaiveDate, bucket: Bucket) -> Result<NaiveDate, CoreError> {
    match bucket {
        Bucket::Day => Ok(date),
        Bucket::Week => date
            .checked_sub_signed(Duration::days(date.weekday().num_days_from_monday() as i64))
            .ok_or(CoreError::InvalidQuery),
        Bucket::Month => date.with_day(1).ok_or(CoreError::InvalidQuery),
    }
}
pub fn normalize_batch(batch: &mut CollectionBatch) -> Result<(), CoreError> {
    for snapshot in &mut batch.snapshots {
        for row in &mut snapshot.rows {
            row.tokens.total.validate()?;
            if row.tokens.total.value.is_none() {
                if let (Some(a), Some(b), Some(c), Some(d)) = (
                    row.tokens.input_uncached.value,
                    row.tokens.cache_read.value,
                    row.tokens.cache_write.value,
                    row.tokens.output_total.value,
                ) {
                    row.tokens.total = Metric {
                        value: Some(
                            a.checked_add(b)
                                .and_then(|s| s.checked_add(c))
                                .and_then(|s| s.checked_add(d))
                                .ok_or(CoreError::Overflow)?,
                        ),
                        accuracy: if [
                            row.tokens.input_uncached.accuracy,
                            row.tokens.cache_read.accuracy,
                            row.tokens.cache_write.accuracy,
                            row.tokens.output_total.accuracy,
                        ]
                        .contains(&Accuracy::Estimated)
                        {
                            Accuracy::Estimated
                        } else {
                            Accuracy::Derived
                        },
                    };
                }
            }
        }
    }
    Ok(())
}
pub fn validate_batch(batch: &CollectionBatch) -> Result<(), CoreError> {
    if batch.snapshots.is_empty() {
        return Err(CoreError::CoverageIncomplete);
    }
    let mut keys = BTreeSet::new();
    for snapshot in &batch.snapshots {
        validate_timezone(&snapshot.key.timezone).map_err(|_| CoreError::InvalidData)?;
        if !keys.insert(&snapshot.key) {
            return Err(CoreError::InvalidData);
        }
        if [
            snapshot.key.product_id.as_str(),
            &snapshot.key.source_dataset_id,
            &snapshot.provider_id,
            &snapshot.origin_device_id,
            &snapshot.collector_version,
            &snapshot.normalization_version,
        ]
        .iter()
        .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err(CoreError::InvalidData);
        }
        if snapshot.collection_started_at > snapshot.collected_at {
            return Err(CoreError::InvalidData);
        }
        if snapshot.warnings.iter().any(|code| {
            code.is_empty()
                || code.len() > 64
                || !code
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        }) {
            return Err(CoreError::InvalidData);
        }
        if snapshot.coverage.state != CoverageState::Complete {
            return Err(CoreError::CoverageIncomplete);
        }
        if let Some(range) = &snapshot.coverage.range {
            range.validate().map_err(|_| CoreError::InvalidData)?;
        }
        if snapshot.key.scope == QueryScope::Standard && snapshot.coverage.range.is_some() {
            return Err(CoreError::CoverageIncomplete);
        }
        if let QueryScope::Filtered {
            start,
            end,
            model_ids,
        } = &snapshot.key.scope
        {
            let range = DateRange {
                start: start.parse().map_err(|_| CoreError::InvalidData)?,
                end: end.parse().map_err(|_| CoreError::InvalidData)?,
            };
            if range.start.to_string() != *start || range.end.to_string() != *end {
                return Err(CoreError::InvalidData);
            }
            range.validate().map_err(|_| CoreError::InvalidData)?;
            if model_ids.windows(2).any(|w| w[0] >= w[1]) {
                return Err(CoreError::InvalidData);
            }
        }
        let mut rows = BTreeSet::new();
        for row in &snapshot.rows {
            if !rows.insert(&row.key) {
                return Err(CoreError::InvalidData);
            }
            if !matches!(
                (&row.key.dimension, snapshot.key.report_kind),
                (RowDimension::Day(_), ReportKind::Daily)
                    | (RowDimension::Session(_), ReportKind::Session)
            ) {
                return Err(CoreError::InvalidData);
            }
            if let RowDimension::Day(date) = row.key.dimension {
                if snapshot
                    .coverage
                    .range
                    .as_ref()
                    .is_some_and(|r| !r.contains(date))
                {
                    return Err(CoreError::InvalidData);
                }
            }
            if let RowDimension::Session(id) = &row.key.dimension {
                if id.is_empty() || id.len() > 256 {
                    return Err(CoreError::InvalidData);
                }
            }
            if row
                .key
                .model_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 256)
            {
                return Err(CoreError::InvalidData);
            }
            for metric in [
                &row.tokens.input_uncached,
                &row.tokens.cache_read,
                &row.tokens.cache_write,
                &row.tokens.output_total,
                &row.tokens.output_reasoning,
                &row.tokens.total,
            ] {
                metric.validate()?;
                if metric.value.is_some_and(|v| v > i64::MAX as u64) {
                    return Err(CoreError::Overflow);
                }
            }
            if let (Some(reasoning), Some(output)) = (
                row.tokens.output_reasoning.value,
                row.tokens.output_total.value,
            ) {
                if reasoning > output {
                    return Err(CoreError::InvalidData);
                }
            }
            if let (Some(input), Some(read), Some(write), Some(output), Some(total)) = (
                row.tokens.input_uncached.value,
                row.tokens.cache_read.value,
                row.tokens.cache_write.value,
                row.tokens.output_total.value,
                row.tokens.total.value,
            ) {
                let reconstructed = input
                    .checked_add(read)
                    .and_then(|v| v.checked_add(write))
                    .and_then(|v| v.checked_add(output))
                    .ok_or(CoreError::Overflow)?;
                if reconstructed != total {
                    return Err(CoreError::InvalidData);
                }
            }
            row.cost.amount_usd.validate()?;
            if row.cost.amount_usd.value.is_some_and(|v| v < Decimal::ZERO) {
                return Err(CoreError::InvalidData);
            }
        }
    }
    Ok(())
}

fn add_metric<T: Copy>(
    target: &mut AggregateMetric<T>,
    metric: &Metric<T>,
    sum: impl Fn(T, T) -> Option<T>,
) -> Result<(), CoreError> {
    metric.validate()?;
    if let Some(value) = metric.value {
        target.metric.value = Some(match target.metric.value {
            Some(current) => sum(current, value).ok_or(CoreError::Overflow)?,
            None => value,
        });
        target.known_rows = target
            .known_rows
            .checked_add(1)
            .ok_or(CoreError::Overflow)?;
        target.metric.accuracy = if target.metric.accuracy == Accuracy::Estimated
            || metric.accuracy == Accuracy::Estimated
        {
            Accuracy::Estimated
        } else if target.known_rows > 1
            || metric.accuracy == Accuracy::Derived
            || target.metric.accuracy == Accuracy::Derived
        {
            Accuracy::Derived
        } else {
            metric.accuracy
        };
    } else {
        target.missing_rows = target
            .missing_rows
            .checked_add(1)
            .ok_or(CoreError::Overflow)?;
    }
    Ok(())
}

fn add_empty_coverage(aggregate: &mut Aggregate, capabilities: &ProviderCapabilities) {
    fn zero<T>(target: &mut AggregateMetric<T>, value: T) {
        if target.metric.value.is_none() && target.missing_rows == 0 {
            target.metric = Metric {
                value: Some(value),
                accuracy: Accuracy::Derived,
            };
        }
    }
    let supports = |metric: &str| capabilities.supported_metrics.iter().any(|m| m == metric);
    // Complete local Daily coverage with no rows proves no total usage, even when a breakdown is unsupported.
    zero(&mut aggregate.tokens.total, 0);
    if supports("input") || supports("input-uncached") {
        zero(&mut aggregate.tokens.input_uncached, 0);
    }
    if supports("output") || supports("output-total") {
        zero(&mut aggregate.tokens.output_total, 0);
    }
    if supports("cache") || supports("cache-read") {
        zero(&mut aggregate.tokens.cache_read, 0);
    }
    if supports("cache") || supports("cache-write") {
        zero(&mut aggregate.tokens.cache_write, 0);
    }
    if supports("reasoning") {
        zero(&mut aggregate.tokens.output_reasoning, 0);
    }
    if supports("cost") {
        zero(&mut aggregate.cost.amount_usd, Decimal::ZERO);
    }
}
pub fn add_row(aggregate: &mut Aggregate, row: &ReportRow) -> Result<(), CoreError> {
    macro_rules! token { ($($field:ident),+) => { $(add_metric(&mut aggregate.tokens.$field, &row.tokens.$field, |a,b| a.checked_add(b).filter(|v| *v <= i64::MAX as u64))?;)+ }; }
    token!(
        input_uncached,
        cache_read,
        cache_write,
        output_total,
        output_reasoning,
        total
    );
    add_metric(
        &mut aggregate.cost.amount_usd,
        &row.cost.amount_usd,
        |a, b| a.checked_add(b),
    )?;
    aggregate
        .cost
        .missing_models
        .extend(row.cost.missing_models.clone());
    aggregate.cost.missing_models.sort();
    aggregate.cost.missing_models.dedup();
    if let Some(version) = &row.cost.pricing_version {
        aggregate.cost.pricing_versions.push(version.clone());
        aggregate.cost.pricing_versions.sort();
        aggregate.cost.pricing_versions.dedup();
    }
    // Unknown pricing dates remain unknown even when some component dates are known.
    aggregate.cost.pricing_as_of = if aggregate.cost.amount_usd.known_rows == 1 {
        row.cost.pricing_as_of
    } else {
        aggregate
            .cost
            .pricing_as_of
            .zip(row.cost.pricing_as_of)
            .map(|(a, b)| a.min(b))
    };
    Ok(())
}
