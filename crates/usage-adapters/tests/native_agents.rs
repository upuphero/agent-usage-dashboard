use chrono::Utc;
use rusqlite::Connection;
use std::{path::PathBuf, sync::Arc};
use usage_adapters::{AgentAdapter, AgentKind, ProcessRunner, SqliteRepository};
use usage_core::*;

struct Now;
impl Clock for Now {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }
}
fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = vec![];
    while value >= 128 {
        bytes.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
fn integer(field: u64, value: u64) -> Vec<u8> {
    [varint(field * 8), varint(value)].concat()
}
fn bytes(field: u64, value: &[u8]) -> Vec<u8> {
    [
        varint(field * 8 + 2),
        varint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}

#[tokio::test]
#[ignore = "requires matching-target pinned ccusage; synthetic usage metadata only"]
async fn codex_response_records_recover_stale_summaries_without_double_counting() {
    let executable =
        PathBuf::from(std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare native sidecar"));
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("sessions")).unwrap();
    let usage = |input: u64| {
        serde_json::json!({
            "input_tokens": input, "cached_input_tokens": 0, "cache_write_input_tokens": 0,
            "output_tokens": 0, "reasoning_output_tokens": 0, "total_tokens": input
        })
    };
    let summary = |timestamp, last, total| {
        serde_json::json!({
            "type": "event_msg", "timestamp": timestamp,
            "payload": {"type": "token_count", "info": {
                "last_token_usage": usage(last), "total_token_usage": usage(total)
            }}
        })
    };
    let record = |id, input, total| {
        serde_json::json!({
            "type": "token_usage_record", "timestamp": "2026-10-08T16:32:25Z",
            "payload": {"response_id": id, "turn_id": "record-turn",
                "usage": usage(input), "thread_token_usage": usage(total)}
        })
    };
    let entries = [
        serde_json::json!({"type": "session_meta", "payload": {"id": "record-session"}}),
        serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5"}}),
        summary("2026-10-07T12:00:00Z", 50, 50),
        record("response-one", 130, 180),
        summary("2026-10-08T16:32:25Z", 130, 180),
        record("response-two", 90, 270),
        // Compaction can leave the legacy summary at the previous response's total.
        serde_json::json!({"type": "compacted", "payload": {"text": "SECRET_BODY"}}),
        summary("2026-10-08T16:32:25Z", 0, 180),
        record("response-two", 90, 270),
        summary("2026-10-08T16:32:25Z", 0, 180),
        // A later legacy-only response must still count, on its local calendar day.
        serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5"}}),
        summary("2026-10-09T07:00:00Z", 40, 310),
    ];
    std::fs::write(
        directory.path().join("sessions/sample.jsonl"),
        entries
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let repository = Arc::new(SqliteRepository::in_memory().unwrap());
    let adapter = Arc::new(
        AgentAdapter::new(
            AgentKind::Codex,
            ProcessRunner::new(&executable, Default::default()).unwrap(),
            "fixture-dataset".into(),
            "fixture-device".into(),
        )
        .unwrap(),
    );
    let service = UsageService::new(repository.clone(), Arc::new(Now), vec![adapter]);
    for index in 0..2 {
        let scan = service
            .run_scan(
                format!("response-scan-{index}"),
                AgentKind::Codex.provider().into(),
                CollectRequest {
                    timezone: "America/Phoenix".into(),
                    config: SourceConfig {
                        enabled: true,
                        root_path: Some(directory.path().to_str().unwrap().into()),
                    },
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(scan.state, ScanState::Succeeded, "{:?}", scan.error);
        let snapshots = repository
            .load_snapshots(SnapshotFilter::default())
            .await
            .unwrap();
        let daily = snapshots
            .iter()
            .find(|s| s.key.report_kind == ReportKind::Daily)
            .unwrap();
        let totals: Vec<_> = daily
            .rows
            .iter()
            .filter(|r| r.key.model_id.is_none())
            .map(|r| match r.key.dimension {
                RowDimension::Day(day) => (day.to_string(), r.tokens.total.value),
                _ => panic!("daily report contained a session row"),
            })
            .collect();
        assert_eq!(
            totals,
            vec![
                ("2026-10-07".into(), Some(50)),
                ("2026-10-08".into(), Some(220)),
                ("2026-10-09".into(), Some(40)),
            ]
        );
        let session = snapshots
            .iter()
            .find(|s| s.key.report_kind == ReportKind::Session)
            .unwrap();
        assert_eq!(
            session
                .rows
                .iter()
                .find(|r| r.key.model_id.is_none())
                .unwrap()
                .tokens
                .total
                .value,
            Some(310)
        );
        assert_eq!(daily.revision, index + 1);
    }
}

#[tokio::test]
#[ignore = "requires matching-target pinned ccusage; synthetic usage metadata only"]
async fn codex_and_antigravity_use_native_reports_replace_snapshots_and_preserve_unknowns() {
    let executable =
        PathBuf::from(std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare native sidecar"));
    for kind in [AgentKind::Codex, AgentKind::Antigravity] {
        let directory = tempfile::tempdir().unwrap();
        let expected = if kind == AgentKind::Codex {
            std::fs::create_dir(directory.path().join("sessions")).unwrap();
            std::fs::write(directory.path().join("sessions/sample.jsonl"), concat!(
                "{\"type\":\"session_meta\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{\"id\":\"fixture-session\"}}\n",
                "{\"type\":\"turn_context\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:00:01Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":20,\"output_tokens\":30,\"reasoning_output_tokens\":10,\"total_tokens\":130}}}}\n",
                "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:01:01Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":180,\"cached_input_tokens\":30,\"output_tokens\":60,\"reasoning_output_tokens\":20,\"total_tokens\":240}}}}\n"
            )).unwrap();
            240
        } else {
            std::fs::create_dir(directory.path().join("conversations")).unwrap();
            let db = Connection::open(directory.path().join("conversations/session.db")).unwrap();
            db.execute_batch("CREATE TABLE gen_metadata(idx INTEGER PRIMARY KEY, data BLOB)")
                .unwrap();
            let usage = [
                integer(2, 100),
                integer(3, 50),
                integer(4, 10),
                integer(5, 50),
                integer(9, 20),
                integer(10, 30),
                bytes(11, b"fixture-response"),
            ]
            .concat();
            let timestamp = [integer(1, 1_791_115_200), integer(2, 0)].concat();
            let chat = [
                bytes(4, &usage),
                bytes(9, &bytes(4, &timestamp)),
                bytes(19, b"Gemini 3 Pro"),
            ]
            .concat();
            db.execute("INSERT INTO gen_metadata VALUES(1,?1)", [bytes(1, &chat)])
                .unwrap();
            210
        };
        let repository = Arc::new(SqliteRepository::in_memory().unwrap());
        let adapter = Arc::new(
            AgentAdapter::new(
                kind,
                ProcessRunner::new(&executable, Default::default()).unwrap(),
                "fixture-dataset".into(),
                "fixture-device".into(),
            )
            .unwrap(),
        );
        let service = UsageService::new(repository.clone(), Arc::new(Now), vec![adapter]);
        for index in 0..3 {
            let scan = service
                .run_scan(
                    format!("scan-{index}"),
                    kind.provider().into(),
                    CollectRequest {
                        timezone: "UTC".into(),
                        config: SourceConfig {
                            enabled: true,
                            root_path: Some(directory.path().to_str().unwrap().into()),
                        },
                    },
                    CancellationToken::default(),
                )
                .await
                .unwrap();
            assert_eq!(scan.state, ScanState::Succeeded, "{:?}", scan.error);
            let snapshots = repository
                .load_snapshots(SnapshotFilter::default())
                .await
                .unwrap();
            assert_eq!(snapshots.len(), 2);
            let daily = snapshots
                .iter()
                .find(|snapshot| snapshot.key.report_kind == ReportKind::Daily)
                .unwrap();
            let parent = daily
                .rows
                .iter()
                .find(|row| row.key.model_id.is_none())
                .unwrap();
            assert_eq!(parent.tokens.total.value, Some(expected));
            assert_eq!(parent.tokens.output_reasoning.value, Some(20));
            assert_eq!(
                parent.tokens.output_total.value,
                Some(if kind == AgentKind::Codex { 60 } else { 50 })
            );
            if kind == AgentKind::Codex {
                assert_eq!(parent.tokens.cache_write.value, None);
            }
            assert_eq!(daily.revision, index + 1);
        }
    }
}
