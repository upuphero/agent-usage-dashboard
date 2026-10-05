//! Immutable, usage-only collection inputs. Never copies authentication or conversation bodies.
use crate::{process::io_error, AgentKind};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use std::{
    fs::{self, File, FileTimes},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
use usage_core::{CancellationToken, CoreError};

pub(crate) struct UsageSnapshot {
    _temporary: tempfile::TempDir,
    pub roots: Vec<PathBuf>,
    pub missing: [bool; 6],
    pub warnings: Vec<String>,
    pub nonzero_usage: bool,
}

pub(crate) fn capture(
    kind: AgentKind,
    roots: &[PathBuf],
    cancel: &CancellationToken,
) -> Result<UsageSnapshot, CoreError> {
    let temporary = tempfile::tempdir().map_err(io_error)?;
    let mut snapshot = UsageSnapshot {
        roots: Vec::new(),
        missing: [false; 6],
        warnings: vec![],
        nonzero_usage: false,
        _temporary: temporary,
    };
    let mut files = 0usize;
    for (index, source) in roots.iter().enumerate() {
        let destination = snapshot._temporary.path().join(format!("source-{index}"));
        fs::create_dir(&destination).map_err(io_error)?;
        snapshot.roots.push(destination.clone());
        let directories = match kind {
            AgentKind::Codex => {
                let found: Vec<_> = ["sessions", "archived_sessions"]
                    .into_iter()
                    .filter(|name| source.join(name).is_dir())
                    .map(|name| (source.join(name), destination.join(name)))
                    .collect();
                if found.is_empty() {
                    vec![(source.clone(), destination)]
                } else {
                    found
                }
            }
            AgentKind::Antigravity => {
                let directory = if source.join("conversations").is_dir() {
                    source.join("conversations")
                } else {
                    source.clone()
                };
                vec![(directory, destination.join("conversations"))]
            }
        };
        let mut queue: Vec<_> = directories.into_iter().map(|(a, b)| (a, b, 0)).collect();
        while let Some((source, destination, depth)) = queue.pop() {
            cancel.check()?;
            if depth > 32
                || fs::symlink_metadata(&source)
                    .map_err(io_error)?
                    .file_type()
                    .is_symlink()
            {
                return Err(CoreError::CoverageIncomplete);
            }
            fs::create_dir_all(&destination).map_err(io_error)?;
            for item in fs::read_dir(&source).map_err(io_error)? {
                cancel.check()?;
                let item = item.map_err(io_error)?;
                let kind_of_file = item.file_type().map_err(io_error)?;
                if kind_of_file.is_symlink() {
                    return Err(CoreError::CoverageIncomplete);
                }
                let path = item.path();
                let output = destination.join(item.file_name());
                if kind_of_file.is_dir() {
                    queue.push((path, output, depth + 1));
                    continue;
                }
                if !kind_of_file.is_file() {
                    continue;
                }
                let extension = path.extension().and_then(|s| s.to_str());
                if kind == AgentKind::Antigravity
                    && extension == Some("pb")
                    && !snapshot
                        .warnings
                        .iter()
                        .any(|w| w == "ANTIGRAVITY_PB_UNSUPPORTED")
                {
                    snapshot.warnings.push("ANTIGRAVITY_PB_UNSUPPORTED".into());
                }
                if extension
                    != Some(if kind == AgentKind::Codex {
                        "jsonl"
                    } else {
                        "db"
                    })
                {
                    continue;
                }
                files += 1;
                if files > 100_000 {
                    return Err(CoreError::CoverageIncomplete);
                }
                match kind {
                    AgentKind::Codex => capture_codex(&path, &output, &mut snapshot, cancel)?,
                    AgentKind::Antigravity => capture_antigravity(&path, &output, cancel)?,
                }
                if let Ok(modified) = item.metadata().map_err(io_error)?.modified() {
                    File::options()
                        .write(true)
                        .open(&output)
                        .map_err(io_error)?
                        .set_times(FileTimes::new().set_modified(modified))
                        .map_err(io_error)?;
                }
            }
        }
    }
    if kind == AgentKind::Antigravity && files == 0 && !snapshot.warnings.is_empty() {
        return Err(CoreError::SchemaUnsupported);
    }
    Ok(snapshot)
}

fn selected(value: &Value, names: &[&str]) -> Value {
    Value::Object(
        names
            .iter()
            .filter_map(|key| value.get(key).map(|v| ((*key).to_owned(), v.clone())))
            .collect(),
    )
}
fn capture_codex(
    source: &Path,
    destination: &Path,
    snapshot: &mut UsageSnapshot,
    cancel: &CancellationToken,
) -> Result<(), CoreError> {
    let file = File::open(source).map_err(io_error)?;
    // A live file is captured at its observed length. Both reports then read this same frozen prefix.
    let length = file.metadata().map_err(io_error)?.len();
    let mut reader = BufReader::new(file.take(length));
    let mut output = File::create(destination).map_err(io_error)?;
    loop {
        cancel.check()?;
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(32 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .map_err(io_error)?;
        if count == 0 {
            break;
        }
        if count > 32 * 1024 * 1024 {
            return Err(CoreError::OutputLimitExceeded);
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_)
                if !line.ends_with(b"\n")
                    && fs::metadata(source).map_err(io_error)?.len() > length =>
            {
                snapshot.warnings.push("ACTIVE_LOG_TAIL_OMITTED".into());
                break;
            }
            Err(_) => return Err(CoreError::CoverageIncomplete),
        };
        let entry = value.get("type").and_then(Value::as_str).unwrap_or("");
        let event = value
            .pointer("/payload/type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let supported = matches!(
            entry,
            "session_meta" | "turn_context" | "token_usage_record" | "compacted" | "turn.completed"
        ) || (entry == "event_msg"
            && matches!(event, "token_count" | "thread_settings_applied"));
        if !supported {
            continue;
        }
        let mut clean = selected(
            &value,
            &[
                "type",
                "timestamp",
                "response_id",
                "turn_id",
                "usage",
                "model",
                "model_name",
            ],
        );
        if let Some(payload) = value.get("payload") {
            let mut clean_payload = selected(
                payload,
                &[
                    "type",
                    "id",
                    "timestamp",
                    "model",
                    "model_name",
                    "forked_from_id",
                    "parent_thread_id",
                    "source",
                    "thread_settings",
                    "service_tier",
                    "response_id",
                    "turn_id",
                    "usage",
                ],
            );
            if let Some(info) = payload.get("info") {
                clean_payload["info"] = selected(
                    info,
                    &[
                        "total_token_usage",
                        "last_token_usage",
                        "model",
                        "model_name",
                    ],
                );
            }
            clean["payload"] = clean_payload;
        }
        if let Some(usage) = value
            .pointer("/payload/info/last_token_usage")
            .filter(|u| u.is_object())
            .or_else(|| {
                value
                    .pointer("/payload/info/total_token_usage")
                    .filter(|u| u.is_object())
            })
            .or_else(|| value.get("usage").filter(|u| u.is_object()))
        {
            let fields: [&[&str]; 6] = [
                &["input_tokens", "prompt_tokens", "input"],
                &[
                    "cached_input_tokens",
                    "cache_read_input_tokens",
                    "cached_tokens",
                ],
                &["cache_creation_input_tokens", "cache_write_input_tokens"],
                &["output_tokens", "completion_tokens", "output"],
                &["reasoning_output_tokens", "reasoning_tokens"],
                &["total_tokens"],
            ];
            for (index, aliases) in fields.iter().enumerate() {
                if !aliases
                    .iter()
                    .any(|name| usage.get(*name).is_some_and(Value::is_u64))
                {
                    snapshot.missing[index] = true;
                }
            }
            snapshot.nonzero_usage |= usage
                .as_object()
                .is_some_and(|values| values.values().any(|v| v.as_u64().is_some_and(|n| n > 0)));
        }
        serde_json::to_writer(&mut output, &clean).map_err(|_| CoreError::InvalidData)?;
        output.write_all(b"\n").map_err(io_error)?;
    }
    Ok(())
}

fn capture_antigravity(
    source: &Path,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<(), CoreError> {
    let mut input = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| CoreError::PermissionDenied)?;
    input
        .busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| CoreError::Storage)?;
    let read = input
        .transaction()
        .map_err(|_| CoreError::CoverageIncomplete)?;
    let mut output = Connection::open(destination).map_err(|_| CoreError::Storage)?;
    let write = output.transaction().map_err(|_| CoreError::Storage)?;
    let has_tables: i64 = read
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::SchemaUnsupported)?;
    for (table, column, schema) in [
        ("gen_metadata", "data", ProtoSchema::Generator),
        ("steps", "metadata", ProtoSchema::Step),
        ("trajectory_metadata_blob", "data", ProtoSchema::Trajectory),
    ] {
        let exists: bool = read
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::SchemaUnsupported)?;
        if !exists {
            if table == "gen_metadata" && has_tables > 0 {
                return Err(CoreError::SchemaUnsupported);
            }
            continue;
        }
        let trajectory = table == "trajectory_metadata_blob";
        let sql = if trajectory {
            "SELECT rowid,data FROM trajectory_metadata_blob ORDER BY rowid".to_owned()
        } else {
            format!("SELECT idx,{column} FROM {table} ORDER BY idx")
        };
        write
            .execute_batch(&format!(
                "CREATE TABLE {table}(idx INTEGER PRIMARY KEY, {column} BLOB)"
            ))
            .map_err(|_| CoreError::Storage)?;
        let mut statement = read
            .prepare(&sql)
            .map_err(|_| CoreError::SchemaUnsupported)?;
        let mut rows = statement
            .query([])
            .map_err(|_| CoreError::CoverageIncomplete)?;
        let mut count = 0usize;
        while let Some(row) = rows.next().map_err(|_| CoreError::CoverageIncomplete)? {
            cancel.check()?;
            count += 1;
            if count > 1_000_000 {
                return Err(CoreError::OutputLimitExceeded);
            }
            let index: i64 = row.get(0).map_err(|_| CoreError::SchemaUnsupported)?;
            let blob: Option<Vec<u8>> = row.get(1).map_err(|_| CoreError::SchemaUnsupported)?;
            let Some(blob) = blob else {
                continue;
            };
            if blob.len() > 32 * 1024 * 1024 {
                return Err(CoreError::OutputLimitExceeded);
            }
            let clean = filter_proto(&blob, schema)?;
            write
                .execute(
                    &format!("INSERT INTO {table}(idx,{column}) VALUES(?1,?2)"),
                    params![index, clean],
                )
                .map_err(|_| CoreError::Storage)?;
        }
    }
    write.commit().map_err(|_| CoreError::Storage)?;
    read.commit().map_err(|_| CoreError::CoverageIncomplete)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ProtoSchema {
    Generator,
    ChatModel,
    Usage,
    Retry,
    GenerationTime,
    Timestamp,
    Step,
    ModelInfo,
    Trajectory,
}
fn varint(bytes: &[u8], offset: &mut usize) -> Result<u64, CoreError> {
    let mut value = 0u64;
    for shift in (0..10).map(|n| n * 7) {
        let byte = *bytes.get(*offset).ok_or(CoreError::SchemaUnsupported)?;
        *offset += 1;
        if shift == 63 && byte & 0x7f > 1 {
            return Err(CoreError::SchemaUnsupported);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(CoreError::SchemaUnsupported)
}
fn put_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 128 {
        output.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    output.push(value as u8);
}
fn filter_proto(bytes: &[u8], schema: ProtoSchema) -> Result<Vec<u8>, CoreError> {
    use ProtoSchema::*;
    let mut offset = 0;
    let mut output = Vec::new();
    while offset < bytes.len() {
        let start = offset;
        let tag = varint(bytes, &mut offset)?;
        let number = tag >> 3;
        let wire = tag & 7;
        if number == 0 || number > u64::from(u32::MAX) {
            return Err(CoreError::SchemaUnsupported);
        }
        let payload_start;
        let length = match wire {
            0 => {
                payload_start = offset;
                let _ = varint(bytes, &mut offset)?;
                offset - payload_start
            }
            1 => {
                payload_start = offset;
                8
            }
            2 => {
                let length = usize::try_from(varint(bytes, &mut offset)?)
                    .map_err(|_| CoreError::SchemaUnsupported)?;
                payload_start = offset;
                length
            }
            5 => {
                payload_start = offset;
                4
            }
            _ => return Err(CoreError::SchemaUnsupported),
        };
        let end = payload_start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(CoreError::SchemaUnsupported)?;
        offset = end;
        let nested = match (schema, number) {
            (Generator, 1) => Some(ChatModel),
            (ChatModel, 4) | (Step, 9) | (Retry, 2) => Some(Usage),
            (ChatModel, 17) | (Step, 28) => Some(Retry),
            (ChatModel, 9) => Some(GenerationTime),
            (GenerationTime, 4) | (Trajectory, 2) | (Step, 1 | 8) => Some(Timestamp),
            (Step, 24) => Some(ModelInfo),
            _ => None,
        };
        if let Some(nested) = nested {
            if wire != 2 {
                return Err(CoreError::SchemaUnsupported);
            }
            let clean = filter_proto(&bytes[payload_start..end], nested)?;
            put_varint(tag, &mut output);
            put_varint(clean.len() as u64, &mut output);
            output.extend(clean);
        } else {
            let keep = match schema {
                ChatModel => matches!(number, 3 | 19 | 21),
                Usage => matches!(number, 1..=7 | 9..=12),
                Timestamp => matches!(number, 1 | 2),
                ModelInfo => matches!(number, 1 | 7 | 8 | 12),
                _ => false,
            };
            if keep {
                output.extend_from_slice(&bytes[start..end]);
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protobuf_filter_keeps_usage_but_discards_body_and_rejects_bad_lengths() {
        let mut chat = vec![];
        put_varint(19 * 8 + 2, &mut chat);
        put_varint(6, &mut chat);
        chat.extend(b"Gemini");
        put_varint(20 * 8 + 2, &mut chat);
        put_varint(6, &mut chat);
        chat.extend(b"SECRET");
        let mut blob = vec![10];
        put_varint(chat.len() as u64, &mut blob);
        blob.extend(chat);
        let clean = filter_proto(&blob, ProtoSchema::Generator).unwrap();
        assert!(clean.windows(6).any(|w| w == b"Gemini"));
        assert!(!clean.windows(6).any(|w| w == b"SECRET"));
        assert!(filter_proto(&[10, 255], ProtoSchema::Generator).is_err());
    }
    #[test]
    fn codex_snapshot_excludes_messages_and_authentication_and_tracks_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        fs::write(dir.path().join("auth.json"), "SECRET_AUTH").unwrap();
        fs::write(dir.path().join("sessions/session.jsonl"), concat!("{\"type\":\"response_item\",\"payload\":{\"text\":\"SECRET_BODY\"}}\n", "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":10,\"output_tokens\":5,\"cached_input_tokens\":2}}}}\n")).unwrap();
        let captured = capture(
            AgentKind::Codex,
            &[dir.path().into()],
            &CancellationToken::default(),
        )
        .unwrap();
        let text = fs::read_to_string(captured.roots[0].join("sessions/session.jsonl")).unwrap();
        assert!(!text.contains("SECRET"));
        assert!(!captured.roots[0].join("auth.json").exists());
        assert!(captured.missing[2]);
        assert!(captured.missing[4]);
        assert!(captured.nonzero_usage);
    }
}
