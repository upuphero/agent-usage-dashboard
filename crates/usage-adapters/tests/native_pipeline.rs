use std::{path::PathBuf, sync::Arc, time::Duration};
use usage_adapters::{ClaudeCodeAdapter, ProcessRunner, RunnerLimits, SqliteRepository};
use usage_core::*;

fn runner(limits: RunnerLimits) -> ProcessRunner {
    ProcessRunner::new(
        PathBuf::from(
            std::env::var_os("CCUSAGE_TEST_BINARY")
                .expect("prepare pinned sidecar and set CCUSAGE_TEST_BINARY"),
        ),
        limits,
    )
    .unwrap()
}
#[tokio::test]
#[ignore = "requires explicitly prepared target-native sidecar; never uses user logs"]
async fn native_collection_mapping_storage_and_query_retains_history_on_failure() {
    let temporary = tempfile::tempdir().unwrap();
    let projects = temporary.path().join("projects/synthetic");
    std::fs::create_dir_all(&projects).unwrap();
    for (name, bytes) in [
        (
            "session-a.jsonl",
            include_bytes!(
                "../../../tests/fixtures/claude-code/logs/projects/synthetic/session-a.jsonl"
            )
            .as_slice(),
        ),
        (
            "session-b.jsonl",
            include_bytes!(
                "../../../tests/fixtures/claude-code/logs/projects/synthetic/session-b.jsonl"
            )
            .as_slice(),
        ),
    ] {
        std::fs::write(projects.join(name), bytes).unwrap();
    }
    let source = Arc::new(
        ClaudeCodeAdapter::new(
            runner(RunnerLimits::default()),
            "dataset-native".into(),
            "device-native".into(),
            Some(temporary.path().to_owned()),
        )
        .unwrap(),
    );
    let repository = Arc::new(SqliteRepository::in_memory().unwrap());
    let now = "2026-10-04T12:00:00Z".parse().unwrap();
    let service = UsageService::new(
        repository.clone(),
        Arc::new(usage_core::memory::FixedClock(now)),
        vec![source],
    );
    let request = || CollectRequest {
        timezone: "America/Phoenix".into(),
        config: SourceConfig {
            enabled: true,
            root_path: None,
        },
    };
    for job in ["01-one", "02-two", "03-three"] {
        assert_eq!(
            service
                .run_scan(
                    job.into(),
                    "ccusage.claude-code".into(),
                    request(),
                    CancellationToken::default()
                )
                .await
                .unwrap()
                .state,
            ScanState::Succeeded
        );
    }
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
        Some(515)
    );
    let original = std::fs::read_to_string(projects.join("session-a.jsonl")).unwrap();
    std::fs::write(
        projects.join("session-a.jsonl"),
        original.replace("\"output_tokens\":40", "\"output_tokens\":45"),
    )
    .unwrap();
    assert_eq!(
        service
            .run_scan(
                "04-corrected".into(),
                "ccusage.claude-code".into(),
                request(),
                CancellationToken::default()
            )
            .await
            .unwrap()
            .state,
        ScanState::Succeeded
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
        Some(520)
    );
    std::fs::write(projects.join("session-a.jsonl"), "{bad tail").unwrap();
    let failed = service
        .run_scan(
            "05-bad-tail".into(),
            "ccusage.claude-code".into(),
            request(),
            CancellationToken::default(),
        )
        .await
        .unwrap();
    assert_eq!(failed.error, Some(CoreError::CoverageIncomplete));
    let cached = service.get_overview(query()).await.unwrap();
    assert!(cached.stale);
    assert_eq!(cached.aggregate.tokens.total.metric.value, Some(520));
    std::fs::remove_file(projects.join("session-a.jsonl")).unwrap();
    assert_eq!(
        service
            .run_scan(
                "06-rotated".into(),
                "ccusage.claude-code".into(),
                request(),
                CancellationToken::default()
            )
            .await
            .unwrap()
            .error,
        Some(CoreError::CoverageIncomplete)
    );
}
#[tokio::test]
#[ignore = "requires explicitly prepared target-native sidecar"]
async fn native_runner_output_limit_timeout_and_cancellation_are_redacted() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/claude-code/logs");
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        runner(RunnerLimits::default())
            .run_report(ReportKind::Daily, &root, "UTC", cancel)
            .await
            .unwrap_err(),
        CoreError::Cancelled
    );
    assert_eq!(
        runner(RunnerLimits {
            timeout: Duration::from_secs(10),
            output_bytes: 1
        })
        .run_report(
            ReportKind::Daily,
            &root,
            "UTC",
            CancellationToken::default()
        )
        .await
        .unwrap_err(),
        CoreError::OutputLimitExceeded
    );
    assert_eq!(
        runner(RunnerLimits {
            timeout: Duration::from_nanos(1),
            output_bytes: 1024 * 1024
        })
        .run_report(
            ReportKind::Daily,
            &root,
            "UTC",
            CancellationToken::default()
        )
        .await
        .unwrap_err(),
        CoreError::Timeout
    );
}
