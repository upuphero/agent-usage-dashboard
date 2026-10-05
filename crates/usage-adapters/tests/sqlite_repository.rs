use chrono::Utc;
use std::sync::Arc;
use usage_adapters::SqliteRepository;
use usage_core::*;

fn batch() -> CollectionBatch {
    CollectionBatch {
        snapshots: vec![ReportSnapshot {
            key: SnapshotKey {
                product_id: "claude-code".into(),
                source_dataset_id: "dataset-sqlite".into(),
                report_kind: ReportKind::Daily,
                timezone: "UTC".into(),
                scope: QueryScope::Standard,
            },
            provider_id: "ccusage.claude-code".into(),
            origin_device_id: "device-sqlite".into(),
            revision: 0,
            collected_at: Utc::now(),
            collection_started_at: Utc::now() - chrono::Duration::seconds(1),
            collector_version: "fixture".into(),
            normalization_version: "1".into(),
            coverage: Coverage {
                state: CoverageState::Complete,
                range: None,
                observed_from: None,
                observed_until: None,
            },
            warnings: vec![],
            rows: vec![ReportRow {
                key: RowKey {
                    dimension: RowDimension::Day("2026-10-04".parse().unwrap()),
                    model_id: None,
                },
                model_vendor: None,
                tokens: TokenMetrics {
                    total: Metric::exact(i64::MAX as u64),
                    ..Default::default()
                },
                cost: CostEstimate::default(),
                session_started_at: None,
                last_activity_at: None,
            }],
        }],
    }
}
#[tokio::test]
async fn signed_64_bit_limits_and_nullable_model_identity_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    let repo = SqliteRepository::open(&path).unwrap();
    repo.commit_batch(batch()).await.unwrap();
    repo.commit_batch(batch()).await.unwrap();
    drop(repo);
    let repo = SqliteRepository::open(&path).unwrap();
    let stored = repo
        .load_snapshots(SnapshotFilter::default())
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].revision, 2);
    assert_eq!(stored[0].rows.len(), 1);
    assert_eq!(stored[0].rows[0].tokens.total.value, Some(i64::MAX as u64));
    let connection = rusqlite::Connection::open(&path).unwrap();
    let (total, storage_type): (i64, String) = connection
        .query_row("SELECT total,typeof(total) FROM report_rows", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(total, i64::MAX);
    assert_eq!(storage_type, "integer");
    assert_eq!(
        connection
            .query_row("SELECT model_key FROM report_rows", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "null"
    );
    let mut overflowing = batch();
    overflowing.snapshots[0].rows[0].tokens.total = Metric::exact(u64::MAX);
    assert_eq!(
        repo.commit_batch(overflowing).await.unwrap_err(),
        CoreError::Overflow
    );
    assert_eq!(
        repo.load_snapshots(SnapshotFilter::default())
            .await
            .unwrap(),
        stored
    );
}
#[test]
fn newer_schema_is_refused_and_failed_migration_rolls_back_and_creates_backup() {
    let dir = tempfile::tempdir().unwrap();
    let future = dir.path().join("future.db");
    let connection = rusqlite::Connection::open(&future).unwrap();
    connection.execute_batch("PRAGMA user_version=2;CREATE TABLE retained(value TEXT);INSERT INTO retained VALUES('keep');").unwrap();
    drop(connection);
    assert!(matches!(
        SqliteRepository::open(&future),
        Err(CoreError::StorageSchemaNewer)
    ));
    let connection = rusqlite::Connection::open(&future).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT value FROM retained", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    let old = dir.path().join("legacy.db");
    let connection = rusqlite::Connection::open(&old).unwrap();
    connection
        .execute_batch("CREATE TABLE devices(legacy TEXT);INSERT INTO devices VALUES('preserve');")
        .unwrap();
    drop(connection);
    assert!(matches!(
        SqliteRepository::open(&old),
        Err(CoreError::Storage)
    ));
    let connection = rusqlite::Connection::open(&old).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='schema_migrations'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT legacy FROM devices", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("legacy.pre-migration-v0-")));
}
#[tokio::test]
async fn independent_writers_increment_one_revision_and_readers_get_complete_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    let a = Arc::new(SqliteRepository::open(&path).unwrap());
    let b = Arc::new(SqliteRepository::open(&path).unwrap());
    let (first, second) = tokio::join!(a.commit_batch(batch()), b.commit_batch(batch()));
    first.unwrap();
    second.unwrap();
    let snapshots = a.load_snapshots(SnapshotFilter::default()).await.unwrap();
    assert_eq!(snapshots[0].revision, 2);
    assert_eq!(snapshots[0].rows.len(), 1);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        a.commit_batch(batch()).await.unwrap_err(),
        CoreError::Storage
    );
    connection.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        a.load_snapshots(SnapshotFilter::default()).await.unwrap(),
        snapshots
    );
}
#[tokio::test]
async fn scan_upsert_and_backup_are_persistent_and_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    let repo = SqliteRepository::open(&path).unwrap();
    let mut scan = ScanRecord {
        job_id: "job".into(),
        provider_id: "ccusage.claude-code".into(),
        state: ScanState::Running,
        started_at: Utc::now(),
        finished_at: None,
        error: None,
        snapshots_replaced: 0,
        rows_written: 0,
    };
    repo.save_scan(scan.clone()).await.unwrap();
    scan.state = ScanState::Failed;
    scan.error = Some(CoreError::Timeout);
    scan.finished_at = Some(Utc::now());
    repo.save_scan(scan.clone()).await.unwrap();
    assert_eq!(
        repo.list_scans("ccusage.claude-code").await.unwrap().len(),
        1
    );
    assert_eq!(
        repo.get_scan("job").await.unwrap().unwrap().error,
        Some(CoreError::Timeout)
    );
    assert!(repo.get_scan("absent").await.unwrap().is_none());
    scan.provider_id = "other-provider".into();
    assert_eq!(
        repo.save_scan(scan).await.unwrap_err(),
        CoreError::DatasetConflict
    );
    let backup = dir.path().join("backup.db");
    repo.backup_to(&backup).unwrap();
    assert_eq!(repo.backup_to(&backup).unwrap_err(), CoreError::Storage);
    let restored = SqliteRepository::open(&backup).unwrap();
    assert_eq!(
        restored.get_scan("job").await.unwrap().unwrap().error,
        Some(CoreError::Timeout)
    );
}
