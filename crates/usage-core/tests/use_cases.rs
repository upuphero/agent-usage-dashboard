use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use usage_core::{
    application::{bucket_start, normalize_batch},
    memory::{FixedClock, MemoryRepository},
    *,
};

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap()
}
fn row(d: &str, model: Option<&str>, total: Option<u64>) -> ReportRow {
    ReportRow {
        key: RowKey {
            dimension: RowDimension::Day(date(d)),
            model_id: model.map(str::to_owned),
        },
        model_vendor: None,
        tokens: TokenMetrics {
            total: total.map(Metric::exact).unwrap_or_default(),
            ..Default::default()
        },
        cost: CostEstimate::default(),
        session_started_at: None,
        last_activity_at: None,
    }
}
fn snapshot(kind: ReportKind, rows: Vec<ReportRow>) -> ReportSnapshot {
    ReportSnapshot {
        key: SnapshotKey {
            product_id: "claude-code".into(),
            source_dataset_id: "dataset-1".into(),
            report_kind: kind,
            timezone: "America/Phoenix".into(),
            scope: QueryScope::Standard,
        },
        provider_id: "ccusage.claude-code".into(),
        origin_device_id: "device-1".into(),
        revision: 0,
        collected_at: now(),
        collection_started_at: now(),
        collector_version: "fixture-1".into(),
        normalization_version: "1".into(),
        coverage: Coverage {
            state: CoverageState::Complete,
            range: None,
            observed_from: None,
            observed_until: None,
        },
        warnings: vec![],
        rows,
    }
}
struct Source(Mutex<Result<CollectionBatch, CoreError>>);
#[async_trait]
impl UsageSource for Source {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: "ccusage.claude-code".into(),
            product_id: "claude-code".into(),
            display_name: "Claude Code".into(),
            capabilities: ProviderCapabilities {
                report_kinds: vec![ReportKind::Daily, ReportKind::Session],
                supported_dimensions: vec!["day".into(), "model".into(), "session".into()],
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
        self.0.lock().unwrap().clone()
    }
}
fn setup(batch: CollectionBatch) -> (UsageService, Arc<MemoryRepository>, Arc<Source>) {
    let repo = Arc::new(MemoryRepository::default());
    let source = Arc::new(Source(Mutex::new(Ok(batch))));
    (
        UsageService::new(
            repo.clone(),
            Arc::new(FixedClock(now())),
            vec![source.clone()],
        ),
        repo,
        source,
    )
}
async fn scan(service: &UsageService, id: &str, cancellation: CancellationToken) -> ScanRecord {
    service
        .run_scan(
            id.into(),
            "ccusage.claude-code".into(),
            CollectRequest {
                timezone: "America/Phoenix".into(),
                config: SourceConfig {
                    enabled: true,
                    root_path: None,
                },
            },
            cancellation,
        )
        .await
        .unwrap()
}
fn query() -> OverviewQuery {
    OverviewQuery {
        range: DateRange {
            start: date("2026-10-01"),
            end: date("2026-10-05"),
        },
        timezone: "America/Phoenix".into(),
        provider_ids: vec![],
        model_ids: vec![],
        bucket: Bucket::Day,
    }
}

#[tokio::test]
async fn three_scans_replace_and_model_change_removes_old_rows() {
    let (service, repo, source) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![
                row("2026-10-04", None, Some(1200)),
                row("2026-10-04", Some("old-model"), Some(1200)),
            ],
        )],
    });
    for i in 0..3 {
        assert_eq!(
            scan(&service, &format!("job-{i}"), CancellationToken::default())
                .await
                .state,
            ScanState::Succeeded
        );
    }
    let result = service.get_overview(query()).await.unwrap();
    assert_eq!(result.aggregate.tokens.total.metric.value, Some(1200));
    assert_eq!(
        repo.load_snapshots(SnapshotFilter::default())
            .await
            .unwrap()[0]
            .revision,
        3
    );
    *source.0.lock().unwrap() = Ok(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![
                row("2026-10-04", None, Some(1300)),
                row("2026-10-04", Some("new-model"), Some(1300)),
            ],
        )],
    });
    scan(&service, "job-4", CancellationToken::default()).await;
    let result = service.get_overview(query()).await.unwrap();
    assert_eq!(result.aggregate.tokens.total.metric.value, Some(1300));
    assert!(!result.by_model.contains_key("old-model"));
}
#[tokio::test]
async fn failure_partial_and_cancellation_preserve_successful_history() {
    let (service, _, source) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![row("2026-10-04", None, Some(42))],
        )],
    });
    scan(&service, "job-0", CancellationToken::default()).await;
    *source.0.lock().unwrap() = Err(CoreError::Timeout);
    assert_eq!(
        scan(&service, "job-1", CancellationToken::default())
            .await
            .error,
        Some(CoreError::Timeout)
    );
    assert!(service.get_overview(query()).await.unwrap().stale);
    let mut partial = snapshot(ReportKind::Daily, vec![]);
    partial.coverage.state = CoverageState::Partial;
    *source.0.lock().unwrap() = Ok(CollectionBatch {
        snapshots: vec![partial],
    });
    assert_eq!(
        scan(&service, "job-2", CancellationToken::default())
            .await
            .error,
        Some(CoreError::CoverageIncomplete)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        scan(&service, "job-3", cancel).await.state,
        ScanState::Cancelled
    );
    assert_eq!(
        service
            .get_overview(query())
            .await
            .unwrap()
            .aggregate
            .tokens
            .total
            .metric
            .value,
        Some(42)
    );
}
#[tokio::test]
async fn daily_session_and_filtered_scopes_never_double_count() {
    let daily = snapshot(ReportKind::Daily, vec![row("2026-10-04", None, Some(100))]);
    let mut session_row = row("2026-10-04", None, Some(900));
    session_row.key.dimension = RowDimension::Session("session-1".into());
    let session = snapshot(ReportKind::Session, vec![session_row]);
    let mut filtered = daily.clone();
    filtered.rows[0].tokens.total = Metric::exact(500);
    filtered.key.scope = QueryScope::Filtered {
        start: "2026-10-01".into(),
        end: "2026-10-05".into(),
        model_ids: vec![],
    };
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![daily, session, filtered],
    });
    scan(&service, "job", CancellationToken::default()).await;
    assert_eq!(
        service
            .get_overview(query())
            .await
            .unwrap()
            .aggregate
            .tokens
            .total
            .metric
            .value,
        Some(100)
    );
    let sessions = service
        .list_sessions(SessionQuery {
            timezone: "America/Phoenix".into(),
            provider_ids: vec![],
            model_ids: vec![],
            active_range: None,
            offset: 0,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(sessions.items[0].usage.tokens.total.metric.value, Some(900));
}
#[tokio::test]
async fn missing_and_zero_remain_distinct_and_end_is_exclusive() {
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![
                row("2026-10-01", None, Some(0)),
                row("2026-10-02", None, None),
                row("2026-10-05", None, Some(999)),
            ],
        )],
    });
    scan(&service, "job", CancellationToken::default()).await;
    let result = service.get_overview(query()).await.unwrap();
    assert_eq!(result.aggregate.tokens.total.metric.value, Some(0));
    assert_eq!(result.aggregate.tokens.total.known_rows, 1);
    assert_eq!(result.aggregate.tokens.total.missing_rows, 1);
    assert_eq!(result.aggregate.tokens.input_uncached.metric.value, None);
}
#[tokio::test]
async fn memory_commit_is_atomic_on_conflict_and_duplicate_rows_are_rejected() {
    let repo = MemoryRepository::default();
    let original = snapshot(ReportKind::Daily, vec![row("2026-10-04", None, Some(100))]);
    repo.commit_batch(CollectionBatch {
        snapshots: vec![original.clone()],
    })
    .await
    .unwrap();
    let mut updated = original.clone();
    updated.rows[0].tokens.total = Metric::exact(999);
    let mut conflict = snapshot(ReportKind::Session, vec![]);
    conflict.origin_device_id = "a".into();
    repo.commit_batch(CollectionBatch {
        snapshots: vec![conflict.clone()],
    })
    .await
    .unwrap();
    conflict.origin_device_id = "b".into();
    assert_eq!(
        repo.commit_batch(CollectionBatch {
            snapshots: vec![updated, conflict]
        })
        .await
        .unwrap_err(),
        CoreError::DatasetConflict
    );
    assert_eq!(
        repo.load_snapshots(SnapshotFilter {
            report_kind: Some(ReportKind::Daily),
            ..Default::default()
        })
        .await
        .unwrap()[0]
            .rows[0]
            .tokens
            .total
            .value,
        Some(100)
    );
    let mut duplicate = original;
    duplicate.rows.push(duplicate.rows[0].clone());
    assert_eq!(
        repo.commit_batch(CollectionBatch {
            snapshots: vec![duplicate]
        })
        .await
        .unwrap_err(),
        CoreError::InvalidData
    );
}
#[test]
fn token_buckets_derive_without_counting_reasoning_twice() {
    let mut r = row("2026-10-04", None, None);
    r.tokens.input_uncached = Metric::exact(400);
    r.tokens.cache_read = Metric::exact(600);
    r.tokens.cache_write = Metric::exact(0);
    r.tokens.output_total = Metric::exact(200);
    r.tokens.output_reasoning = Metric::exact(50);
    let mut batch = CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Daily, vec![r])],
    };
    normalize_batch(&mut batch).unwrap();
    assert_eq!(batch.snapshots[0].rows[0].tokens.total.value, Some(1200));
    assert_eq!(
        batch.snapshots[0].rows[0].tokens.total.accuracy,
        Accuracy::Derived
    );
}
#[test]
fn iso_week_and_month_cross_year_boundaries() {
    assert_eq!(
        bucket_start(date("2021-01-01"), Bucket::Week).unwrap(),
        date("2020-12-28")
    );
    assert_eq!(
        bucket_start(date("2026-10-31"), Bucket::Month).unwrap(),
        date("2026-10-01")
    );
}
#[tokio::test]
async fn session_activity_filter_uses_iana_zone_across_dst_and_paginates() {
    let mut r = row("2026-10-04", None, Some(77));
    r.key.dimension = RowDimension::Session("s1".into());
    r.last_activity_at = Some(Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap());
    let mut r2 = r.clone();
    r2.key.dimension = RowDimension::Session("s2".into());
    r2.last_activity_at = Some(Utc.with_ymd_and_hms(2026, 11, 1, 6, 30, 0).unwrap());
    let mut snap = snapshot(ReportKind::Session, vec![r, r2]);
    snap.key.timezone = "America/New_York".into();
    let (service, repo, _) = setup(CollectionBatch { snapshots: vec![] });
    repo.commit_batch(CollectionBatch {
        snapshots: vec![snap],
    })
    .await
    .unwrap();
    let q = SessionQuery {
        timezone: "America/New_York".into(),
        provider_ids: vec![],
        model_ids: vec![],
        active_range: Some(DateRange {
            start: date("2026-11-01"),
            end: date("2026-11-02"),
        }),
        offset: 0,
        limit: 1,
    };
    let result = service.list_sessions(q).await.unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.next_offset, Some(1));
    assert_eq!(result.items[0].usage.tokens.total.metric.value, Some(77));
}
#[tokio::test]
async fn providers_do_not_invent_data_and_invalid_queries_fail() {
    let (service, _, _) = setup(CollectionBatch { snapshots: vec![] });
    let result = service.get_overview(query()).await.unwrap();
    assert_eq!(result.aggregate.tokens.total.metric.value, None);
    let mut q = query();
    q.timezone = "Invalid/Timezone".into();
    assert_eq!(
        service.get_overview(q).await.unwrap_err(),
        CoreError::InvalidQuery
    );
    assert!(!service.list_providers(&BTreeMap::new()).await.unwrap()[0].enabled);
}

#[tokio::test]
async fn known_counts_overflow_safely_and_unavailable_model_breakdown_rejects_filter() {
    let (service, repo, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![
                row("2026-10-01", None, Some(i64::MAX as u64)),
                row("2026-10-02", None, Some(1)),
            ],
        )],
    });
    scan(&service, "job", CancellationToken::default()).await;
    assert_eq!(
        service.get_overview(query()).await.unwrap_err(),
        CoreError::Overflow
    );
    let mut q = query();
    q.model_ids = vec!["unknown".into()];
    assert_eq!(
        service.get_overview(q).await.unwrap_err(),
        CoreError::UnsupportedFilter
    );
    assert_eq!(
        repo.load_snapshots(SnapshotFilter::default())
            .await
            .unwrap()[0]
            .rows
            .len(),
        2
    );
}

#[tokio::test]
async fn complete_success_empty_is_zero_but_unsupported_metrics_stay_unavailable() {
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Daily, vec![])],
    });
    scan(&service, "job", CancellationToken::default()).await;
    let overview = service.get_overview(query()).await.unwrap();
    assert_eq!(overview.aggregate.tokens.total.metric.value, Some(0));
    assert_eq!(overview.aggregate.tokens.cache_read.metric.value, None);
    assert!(!overview.coverage.is_empty());
    assert_eq!(overview.buckets.len(), 4);
    assert!(overview
        .buckets
        .values()
        .all(|bucket| bucket.tokens.total.metric.value == Some(0)));
}

#[tokio::test]
async fn complete_daily_timeline_fills_no_usage_days_and_keeps_explicit_unknown() {
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![
                row("2026-10-01", None, Some(10)),
                row("2026-10-03", None, None),
                row("2026-10-04", None, Some(20)),
            ],
        )],
    });
    scan(&service, "timeline", CancellationToken::default()).await;
    let overview = service.get_overview(query()).await.unwrap();
    assert_eq!(overview.buckets.len(), 4);
    assert_eq!(
        overview.buckets[&date("2026-10-02")]
            .tokens
            .total
            .metric
            .value,
        Some(0)
    );
    assert_eq!(
        overview.buckets[&date("2026-10-03")]
            .tokens
            .total
            .metric
            .value,
        None
    );
    assert_eq!(overview.aggregate.tokens.total.metric.value, Some(30));
    assert_eq!(overview.aggregate.tokens.total.known_rows, 2);
    assert_eq!(overview.aggregate.tokens.total.missing_rows, 1);
}

#[tokio::test]
async fn empty_cache_is_not_a_zero_timeline_and_week_month_use_zero_buckets() {
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Daily, vec![])],
    });
    assert!(service
        .get_overview(query())
        .await
        .unwrap()
        .buckets
        .is_empty());
    scan(&service, "empty-periods", CancellationToken::default()).await;
    let mut q = query();
    q.range = DateRange {
        start: date("2026-09-28"),
        end: date("2026-10-12"),
    };
    q.bucket = Bucket::Week;
    let weeks = service.get_overview(q.clone()).await.unwrap();
    assert_eq!(weeks.buckets.len(), 2);
    assert_eq!(
        weeks.buckets[&date("2026-10-05")].tokens.total.metric.value,
        Some(0)
    );
    q.bucket = Bucket::Month;
    let months = service.get_overview(q).await.unwrap();
    assert_eq!(months.buckets.len(), 2);
    assert_eq!(
        months.buckets[&date("2026-10-01")]
            .tokens
            .total
            .metric
            .value,
        Some(0)
    );
}

#[tokio::test]
async fn zero_timeline_respects_declared_coverage_boundaries() {
    let mut bounded = snapshot(ReportKind::Daily, vec![row("2026-10-01", None, Some(5))]);
    bounded.coverage.range = Some(DateRange {
        start: date("2026-10-01"),
        end: date("2026-10-03"),
    });
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![bounded],
    });
    scan(&service, "bounded-timeline", CancellationToken::default()).await;
    let result = service.get_overview(query()).await.unwrap();
    assert_eq!(result.buckets.len(), 2);
    assert_eq!(
        result.buckets[&date("2026-10-02")]
            .tokens
            .total
            .metric
            .value,
        Some(0)
    );
    assert!(!result.buckets.contains_key(&date("2026-10-03")));
}

#[tokio::test]
async fn disappearing_history_never_turns_successful_cache_into_zero() {
    let (service, _, source) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![row("2026-10-04", None, Some(42))],
        )],
    });
    scan(&service, "job-0", CancellationToken::default()).await;
    *source.0.lock().unwrap() = Ok(CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Daily, vec![])],
    });
    assert_eq!(
        scan(&service, "job-1", CancellationToken::default())
            .await
            .error,
        Some(CoreError::CoverageIncomplete)
    );
    let overview = service.get_overview(query()).await.unwrap();
    assert_eq!(overview.aggregate.tokens.total.metric.value, Some(42));
    assert!(overview.stale);
}

fn session_row(id: &str, model: Option<&str>, total: Option<u64>) -> ReportRow {
    let mut value = row("2026-10-04", model, total);
    value.key.dimension = RowDimension::Session(id.into());
    value.last_activity_at = Some(now());
    value
}
fn session_query() -> SessionQuery {
    SessionQuery {
        timezone: "America/Phoenix".into(),
        provider_ids: vec![],
        model_ids: vec![],
        active_range: None,
        offset: 0,
        limit: 1,
    }
}

#[tokio::test]
async fn session_models_without_parent_form_one_item_and_keep_partial_fields() {
    let a = session_row("session-1", Some("model-a"), Some(10));
    let mut b = session_row("session-1", Some("model-b"), Some(20));
    b.tokens.output_total = Metric::exact(4);
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Session, vec![a, b])],
    });
    scan(&service, "job", CancellationToken::default()).await;
    let page = service.list_sessions(session_query()).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.next_offset, None);
    let item = &page.items[0];
    assert_eq!(item.session_id, "session-1");
    assert_eq!(item.model_id, None);
    assert_eq!(item.usage.tokens.total.metric.value, Some(30));
    assert_eq!(item.usage.tokens.output_total.metric.value, Some(4));
    assert_eq!(item.usage.tokens.output_total.missing_rows, 1);
    assert_eq!(item.session_started_at, None);
}

#[tokio::test]
async fn session_parent_is_authoritative_and_model_filter_uses_whole_session_activity() {
    let parent = session_row("session-1", None, Some(30));
    let mut a = session_row("session-1", Some("model-a"), Some(10));
    a.last_activity_at = Some(now() - chrono::Duration::days(1));
    let b = session_row("session-1", Some("model-b"), Some(20));
    let (service, _, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(ReportKind::Session, vec![parent, a, b])],
    });
    scan(&service, "job", CancellationToken::default()).await;
    let page = service.list_sessions(session_query()).await.unwrap();
    assert_eq!(page.items[0].usage.tokens.total.metric.value, Some(30));
    assert_eq!(page.items[0].usage.tokens.total.known_rows, 1);
    let mut query = session_query();
    query.model_ids = vec!["model-a".into()];
    query.active_range = Some(DateRange {
        start: date("2026-10-04"),
        end: date("2026-10-05"),
    });
    let filtered = service.list_sessions(query).await.unwrap();
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].usage.tokens.total.metric.value, Some(10));
    assert_eq!(filtered.items[0].last_activity_at, Some(now()));
}

#[tokio::test]
async fn missing_session_models_reject_filter_and_queued_start_is_preserved() {
    let (service, repo, _) = setup(CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Session,
            vec![session_row("session-1", None, Some(30))],
        )],
    });
    let queued_at = now() - chrono::Duration::seconds(5);
    repo.save_scan(ScanRecord {
        job_id: "job".into(),
        provider_id: "ccusage.claude-code".into(),
        state: ScanState::Queued,
        started_at: queued_at,
        finished_at: None,
        error: None,
        snapshots_replaced: 0,
        rows_written: 0,
    })
    .await
    .unwrap();
    let result = scan(&service, "job", CancellationToken::default()).await;
    assert_eq!(result.started_at, queued_at);
    let mut query = session_query();
    query.model_ids = vec!["model-a".into()];
    assert_eq!(
        service.list_sessions(query).await.unwrap_err(),
        CoreError::UnsupportedFilter
    );
    assert_eq!(
        service
            .run_scan(
                "job".into(),
                "ccusage.claude-code".into(),
                CollectRequest {
                    timezone: "America/Phoenix".into(),
                    config: SourceConfig {
                        enabled: true,
                        root_path: None
                    }
                },
                CancellationToken::default()
            )
            .await
            .unwrap_err(),
        CoreError::InvalidQuery
    );
    assert_eq!(
        repo.get_scan("job").await.unwrap().unwrap().state,
        ScanState::Succeeded
    );
}

#[tokio::test]
async fn latest_scan_follows_observation_order_when_clock_and_opaque_ids_do_not() {
    let original = CollectionBatch {
        snapshots: vec![snapshot(
            ReportKind::Daily,
            vec![row("2026-10-04", None, Some(42))],
        )],
    };
    let (service, _, source) = setup(original.clone());
    scan(&service, "z-first", CancellationToken::default()).await;
    *source.0.lock().unwrap() = Err(CoreError::Timeout);
    scan(&service, "a-failed", CancellationToken::default()).await;
    assert!(service.get_overview(query()).await.unwrap().stale);
    *source.0.lock().unwrap() = Ok(original);
    scan(&service, "0-recovered", CancellationToken::default()).await;
    assert!(!service.get_overview(query()).await.unwrap().stale);
    assert_eq!(
        service.list_providers(&BTreeMap::new()).await.unwrap()[0]
            .last_scan
            .as_ref()
            .unwrap()
            .job_id,
        "0-recovered"
    );
}
