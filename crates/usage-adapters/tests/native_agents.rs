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
async fn codex_and_antigravity_use_native_reports_replace_snapshots_and_preserve_unknowns() {
    let executable =
        PathBuf::from(std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare native sidecar"));
    for kind in [AgentKind::Codex, AgentKind::Antigravity] {
        let directory = tempfile::tempdir().unwrap();
        let expected;
        if kind == AgentKind::Codex {
            std::fs::create_dir(directory.path().join("sessions")).unwrap();
            std::fs::write(directory.path().join("sessions/sample.jsonl"), concat!(
                "{\"type\":\"session_meta\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{\"id\":\"fixture-session\"}}\n",
                "{\"type\":\"turn_context\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:00:01Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":20,\"output_tokens\":30,\"reasoning_output_tokens\":10,\"total_tokens\":130}}}}\n",
                "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:01:01Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":180,\"cached_input_tokens\":30,\"output_tokens\":60,\"reasoning_output_tokens\":20,\"total_tokens\":240}}}}\n"
            )).unwrap();
            expected = 240;
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
            expected = 210;
        }
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
