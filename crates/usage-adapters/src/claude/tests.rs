use super::*;
use crate::SqliteRepository;
use std::sync::Arc;

fn fixture(kind: ReportKind) -> ReportSnapshot {
    let bytes = match kind {
        ReportKind::Daily => {
            include_bytes!("../../../../tests/fixtures/claude-code/daily.json").as_slice()
        }
        ReportKind::Session => {
            include_bytes!("../../../../tests/fixtures/claude-code/session.json").as_slice()
        }
    };
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    decode_report(
        bytes,
        kind,
        "America/Phoenix",
        "dataset-1",
        "device-1",
        now,
        now,
    )
    .unwrap()
}
fn batch() -> CollectionBatch {
    CollectionBatch {
        snapshots: vec![fixture(ReportKind::Daily), fixture(ReportKind::Session)],
    }
}
#[test]
fn mapping_has_disjoint_cache_and_unknown_reasoning_and_prices() {
    let snapshot = fixture(ReportKind::Daily);
    assert_eq!(snapshot.rows.len(), 5);
    let first = &snapshot.rows[0];
    assert_eq!(first.tokens.input_uncached.value, Some(100));
    assert_eq!(first.tokens.cache_read.value, Some(50));
    assert_eq!(first.tokens.cache_write.value, Some(30));
    assert_eq!(first.tokens.total.value, Some(200));
    assert_eq!(first.tokens.output_reasoning, Metric::unavailable());
    let unknown = snapshot
        .rows
        .iter()
        .find(|r| r.key.model_id.as_deref() == Some("synthetic-unpriced-model"))
        .unwrap();
    assert_eq!(unknown.cost.amount_usd, Metric::unavailable());
    assert_eq!(
        unknown.cost.missing_models,
        vec!["synthetic-unpriced-model"]
    );
    let known = snapshot
        .rows
        .iter()
        .find(|r| r.key.model_id.as_deref() == Some("claude-sonnet-4-20250514"))
        .unwrap();
    assert_eq!(known.cost.amount_usd.accuracy, Accuracy::Estimated);
    assert_eq!(known.cost.pricing_as_of, None);
    let sessions = fixture(ReportKind::Session);
    assert!(sessions
        .rows
        .iter()
        .filter(|r| r.key.model_id.is_some())
        .all(|r| r.session_started_at.is_none() && r.last_activity_at.is_none()));
    assert_eq!(sessions.rows[0].tokens.total.value, Some(500));
    assert_eq!(
        sessions.rows[0].session_started_at.unwrap().to_rfc3339(),
        "2026-10-04T06:59:00+00:00"
    );
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/claude-code/session.json"
    ))
    .unwrap();
    value["sessions"][1]
        .as_object_mut()
        .unwrap()
        .remove("modelsUsed");
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    let sparse = decode_report(
        &serde_json::to_vec(&value).unwrap(),
        ReportKind::Session,
        "America/Phoenix",
        "dataset-1",
        "device-1",
        now,
        now,
    )
    .unwrap();
    assert_eq!(
        sparse
            .rows
            .iter()
            .find(|row| row.key.model_id.is_none()
                && row.key.dimension == RowDimension::Session("session-b".into()))
            .unwrap()
            .cost
            .amount_usd,
        Metric::unavailable()
    );
}
#[test]
fn unknown_fields_are_dropped_and_missing_values_are_not_zero() {
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/claude-code/daily.json"
    ))
    .unwrap();
    value["daily"][0]["inputTokens"] = serde_json::Value::Null;
    value["daily"][0]["title"] = "DO_NOT_STORE_BODY_OR_AUTH".into();
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    let mapped = decode_report(
        &serde_json::to_vec(&value).unwrap(),
        ReportKind::Daily,
        "America/Phoenix",
        "dataset-1",
        "device-1",
        now,
        now,
    )
    .unwrap();
    assert_eq!(mapped.rows[0].tokens.input_uncached, Metric::unavailable());
    assert!(!serde_json::to_string(&mapped)
        .unwrap()
        .contains("DO_NOT_STORE"));
    value["daily"][0]["inputTokens"] = (-1).into();
    assert_eq!(
        decode_report(
            &serde_json::to_vec(&value).unwrap(),
            ReportKind::Daily,
            "America/Phoenix",
            "dataset-1",
            "device-1",
            now,
            now
        )
        .unwrap_err(),
        CoreError::SchemaUnsupported
    );
}
#[test]
fn corrupted_report_shape_or_overflow_is_rejected() {
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    for bytes in [
        b"not json".as_slice(),
        br#"{"type":"daily","data":[]}"#.as_slice(),
    ] {
        assert_eq!(
            decode_report(
                bytes,
                ReportKind::Daily,
                "UTC",
                "dataset-1",
                "device-1",
                now,
                now
            )
            .unwrap_err(),
            CoreError::SchemaUnsupported
        );
    }
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/claude-code/daily.json"
    ))
    .unwrap();
    value["daily"][0]["totalTokens"] = u64::MAX.into();
    assert_eq!(
        decode_report(
            &serde_json::to_vec(&value).unwrap(),
            ReportKind::Daily,
            "UTC",
            "dataset-1",
            "device-1",
            now,
            now
        )
        .unwrap_err(),
        CoreError::Overflow
    );
}
#[test]
fn source_audit_catches_bad_tail_and_missing_buckets_without_storing_body() {
    let temporary = tempfile::tempdir().unwrap();
    let projects = temporary.path().join("projects");
    std::fs::create_dir(&projects).unwrap();
    let file = projects.join("a.jsonl");
    std::fs::write(&file, "{\"timestamp\":\"2026-10-04T00:00:00Z\",\"message\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n").unwrap();
    let audit = audit_source(temporary.path(), &CancellationToken::default()).unwrap();
    assert_eq!(audit.missing_buckets, [false, false, true, true]);
    assert_eq!(audit.usage_lines, 1);
    std::fs::write(&file, "{\"message\":").unwrap();
    assert_eq!(
        audit_source(temporary.path(), &CancellationToken::default()).unwrap_err(),
        CoreError::CoverageIncomplete
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        audit_source(temporary.path(), &cancelled).unwrap_err(),
        CoreError::Cancelled
    );
}

struct FixtureSource;
#[async_trait]
impl UsageSource for FixtureSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: PROVIDER.into(),
            product_id: PRODUCT.into(),
            display_name: "Claude Code fixture".into(),
            capabilities: ProviderCapabilities {
                report_kinds: vec![ReportKind::Daily, ReportKind::Session],
                supported_dimensions: vec!["day".into(), "session".into(), "model".into()],
                supported_metrics: vec!["total".into()],
                supports_date_session_intersection: false,
                supports_incremental_collection: false,
                supports_quota: false,
            },
        }
    }
    async fn detect(&self, _: &SourceConfig) -> Result<Detection, CoreError> {
        Ok(Detection {
            state: ProviderState::Ready,
            path_hint: None,
        })
    }
    async fn collect(
        &self,
        _: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<CollectionBatch, CoreError> {
        cancellation.check()?;
        Ok(batch())
    }
}

struct MutableSource(std::sync::Mutex<Result<CollectionBatch, CoreError>>);
#[async_trait]
impl UsageSource for MutableSource {
    fn descriptor(&self) -> ProviderDescriptor {
        FixtureSource.descriptor()
    }
    async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError> {
        FixtureSource.detect(config).await
    }
    async fn collect(
        &self,
        _: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<CollectionBatch, CoreError> {
        cancellation.check()?;
        self.0.lock().unwrap().clone()
    }
}
#[tokio::test]
async fn failed_or_emptied_source_retains_history_and_marks_queries_stale() {
    let repository = Arc::new(SqliteRepository::in_memory().unwrap());
    let source = Arc::new(MutableSource(std::sync::Mutex::new(Ok(batch()))));
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    let service = UsageService::new(
        repository.clone(),
        Arc::new(usage_core::memory::FixedClock(now)),
        vec![source.clone()],
    );
    let request = || CollectRequest {
        timezone: "America/Phoenix".into(),
        config: SourceConfig {
            enabled: true,
            root_path: None,
        },
    };
    assert_eq!(
        service
            .run_scan(
                "a-good".into(),
                PROVIDER.into(),
                request(),
                CancellationToken::default()
            )
            .await
            .unwrap()
            .state,
        ScanState::Succeeded
    );
    let previous = repository
        .load_snapshots(SnapshotFilter::default())
        .await
        .unwrap();
    *source.0.lock().unwrap() = Err(CoreError::SchemaUnsupported);
    assert_eq!(
        service
            .run_scan(
                "b-failed".into(),
                PROVIDER.into(),
                request(),
                CancellationToken::default()
            )
            .await
            .unwrap()
            .error,
        Some(CoreError::SchemaUnsupported)
    );
    assert_eq!(
        repository
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        previous
    );
    let query = || OverviewQuery {
        range: DateRange {
            start: "2026-10-03".parse().unwrap(),
            end: "2026-10-05".parse().unwrap(),
        },
        timezone: "America/Phoenix".into(),
        provider_ids: vec![],
        model_ids: vec![],
        bucket: Bucket::Day,
    };
    let stale = service.get_overview(query()).await.unwrap();
    assert!(stale.stale);
    assert_eq!(stale.aggregate.tokens.total.metric.value, Some(515));
    let mut empty = batch();
    for snapshot in &mut empty.snapshots {
        snapshot.rows.clear();
    }
    *source.0.lock().unwrap() = Ok(empty);
    assert_eq!(
        service
            .run_scan(
                "c-rotated".into(),
                PROVIDER.into(),
                request(),
                CancellationToken::default()
            )
            .await
            .unwrap()
            .error,
        Some(CoreError::CoverageIncomplete)
    );
    assert_eq!(
        repository
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        previous
    );
}
#[tokio::test]
async fn fixture_mapping_sqlite_and_core_queries_do_not_add_daily_session_or_model_totals() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("usage.db");
    let repository = Arc::new(SqliteRepository::open(&path).unwrap());
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    let service = UsageService::new(
        repository.clone(),
        Arc::new(usage_core::memory::FixedClock(now)),
        vec![Arc::new(FixtureSource)],
    );
    for job in ["scan-1", "scan-2", "scan-3"] {
        let scan = service
            .run_scan(
                job.into(),
                PROVIDER.into(),
                CollectRequest {
                    timezone: "America/Phoenix".into(),
                    config: SourceConfig {
                        enabled: true,
                        root_path: None,
                    },
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(scan.state, ScanState::Succeeded);
        assert_eq!(scan.rows_written, 9);
    }
    let result = service
        .get_overview(OverviewQuery {
            range: DateRange {
                start: "2026-10-03".parse().unwrap(),
                end: "2026-10-05".parse().unwrap(),
            },
            timezone: "America/Phoenix".into(),
            provider_ids: vec![],
            model_ids: vec![],
            bucket: Bucket::Day,
        })
        .await
        .unwrap();
    assert_eq!(result.aggregate.tokens.total.metric.value, Some(515));
    assert_eq!(
        result.by_model["claude-sonnet-4-20250514"]
            .tokens
            .total
            .metric
            .value,
        Some(500)
    );
    assert_eq!(
        result.by_model["synthetic-unpriced-model"]
            .cost
            .amount_usd
            .metric
            .value,
        None
    );
    let sessions = service
        .list_sessions(SessionQuery {
            timezone: "America/Phoenix".into(),
            provider_ids: vec![],
            model_ids: vec![],
            active_range: None,
            offset: 0,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(sessions.total, 2);
    let stored = repository
        .load_snapshots(SnapshotFilter::default())
        .await
        .unwrap();
    assert!(stored.iter().all(|s| s.revision == 3));
    assert_eq!(stored.iter().map(|s| s.rows.len()).sum::<usize>(), 9);
    repository
        .backup_to(temporary.path().join("backup.db"))
        .unwrap();
    let restored = SqliteRepository::open(temporary.path().join("backup.db")).unwrap();
    assert_eq!(
        restored
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        stored
    );
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(11).any(|w| w == b"projectPath"));
}

#[tokio::test]
async fn correction_replaces_old_rows_and_conflicting_batch_rolls_back_every_snapshot() {
    let repository = SqliteRepository::in_memory().unwrap();
    repository.commit_batch(batch()).await.unwrap();
    let mut corrected = batch();
    let snapshot = &mut corrected.snapshots[0];
    // A changed model split is a replacement, not another generation to add.
    snapshot
        .rows
        .retain(|r| r.key.model_id.as_deref() != Some("synthetic-unpriced-model"));
    let parent = snapshot
        .rows
        .iter_mut()
        .find(|r| r.key.model_id.is_none() && r.tokens.total.value == Some(315))
        .unwrap();
    parent.tokens.output_total = Metric::exact(50);
    parent.tokens.total = Metric::exact(320);
    repository.commit_batch(corrected).await.unwrap();
    let prior = repository
        .load_snapshots(SnapshotFilter::default())
        .await
        .unwrap();
    assert_eq!(prior[0].rows.len(), 4);
    assert_eq!(prior[0].revision, 2);
    let mut conflict = batch();
    conflict.snapshots[1].origin_device_id = "other-device".into();
    assert_eq!(
        repository.commit_batch(conflict).await.unwrap_err(),
        CoreError::DatasetConflict
    );
    assert_eq!(
        repository
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        prior
    );
    let mut partial = batch();
    partial.snapshots[0].coverage.state = CoverageState::Partial;
    assert_eq!(
        repository.commit_batch(partial).await.unwrap_err(),
        CoreError::CoverageIncomplete
    );
    assert_eq!(
        repository
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        prior
    );
}
