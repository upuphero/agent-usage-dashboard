//! Shared input boundaries for collection, metadata checks and OS hints. No bodies/auth read.
use crate::{process::io_error, AgentKind};
use notify::{event::ModifyKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use usage_core::{CancellationToken, ChangeCallback, CoreError, SourceObservation, SourceWatch};

#[derive(Clone, Copy)]
pub(crate) enum Layout {
    Claude,
    Agent(AgentKind),
}
pub(crate) fn directories(layout: Layout, root: &Path) -> Vec<PathBuf> {
    match layout {
        Layout::Claude => vec![root.join("projects")],
        Layout::Agent(AgentKind::Codex) => {
            let found: Vec<_> = ["sessions", "archived_sessions"]
                .into_iter()
                .map(|name| root.join(name))
                .filter(|path| path.is_dir())
                .collect();
            if found.is_empty() {
                vec![root.to_owned()]
            } else {
                found
            }
        }
        Layout::Agent(AgentKind::Antigravity) => {
            vec![if root.join("conversations").is_dir() {
                root.join("conversations")
            } else {
                root.to_owned()
            }]
        }
    }
}
fn accepted(layout: Layout, path: &Path) -> bool {
    match layout {
        Layout::Claude | Layout::Agent(AgentKind::Codex) => {
            path.extension().is_some_and(|s| s == "jsonl")
        }
        Layout::Agent(AgentKind::Antigravity) => {
            path.extension().is_some_and(|s| s == "db" || s == "pb")
                || path
                    .file_name()
                    .is_some_and(|s| s.to_string_lossy().ends_with(".db-wal"))
        }
    }
}
pub(crate) fn inspect(
    layout: Layout,
    roots: &[PathBuf],
    dataset: &str,
    timezone: &str,
    version: &str,
    cancellation: &CancellationToken,
) -> Result<SourceObservation, CoreError> {
    let provider = match layout {
        Layout::Claude => "ccusage.claude-code",
        Layout::Agent(kind) => kind.provider(),
    };
    let scope = format!("{provider}|{dataset}|{timezone}|{version}|{roots:?}");
    let mut files = std::collections::BTreeMap::new();
    for root in roots {
        let mut queue: Vec<_> = directories(layout, root)
            .into_iter()
            .map(|p| (p, 0))
            .collect();
        while let Some((directory, depth)) = queue.pop() {
            cancellation.check()?;
            if depth > 32
                || fs::symlink_metadata(&directory)
                    .map_err(io_error)?
                    .file_type()
                    .is_symlink()
            {
                return Err(CoreError::CoverageIncomplete);
            }
            for entry in fs::read_dir(&directory).map_err(io_error)? {
                cancellation.check()?;
                let entry = entry.map_err(io_error)?;
                let kind = entry.file_type().map_err(io_error)?;
                if kind.is_symlink() {
                    return Err(CoreError::CoverageIncomplete);
                }
                let path = entry.path();
                if kind.is_dir() {
                    queue.push((path, depth + 1));
                    continue;
                }
                if !kind.is_file() || !accepted(layout, &path) {
                    continue;
                }
                if path
                    .file_name()
                    .is_some_and(|s| s.to_string_lossy().ends_with(".db-wal"))
                    && !path
                        .with_file_name(
                            path.file_name()
                                .unwrap()
                                .to_string_lossy()
                                .trim_end_matches("-wal"),
                        )
                        .is_file()
                {
                    continue;
                }
                if files.len() >= 100_000 {
                    return Err(CoreError::CoverageIncomplete);
                }
                let metadata = fs::metadata(&path).map_err(io_error)?;
                files.insert(
                    path.clone(),
                    format!(
                        "{}|{:?}|{}",
                        metadata.len(),
                        metadata.modified().map_err(io_error)?,
                        identity(&path, &metadata)?
                    ),
                );
            }
        }
    }
    let mut hash = Sha256::new();
    for (path, metadata) in files {
        hash.update(format!("{path:?}|{metadata}\n"));
    }
    Ok(SourceObservation {
        scope,
        fingerprint: format!("{:x}", hash.finalize()),
    })
}
#[cfg(unix)]
fn identity(_: &Path, metadata: &fs::Metadata) -> Result<String, CoreError> {
    use std::os::unix::fs::MetadataExt;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}
#[cfg(windows)]
fn identity(path: &Path, _: &fs::Metadata) -> Result<String, CoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let file = fs::File::open(path).map_err(io_error)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}
struct NativeWatch {
    _watcher: RecommendedWatcher,
}
impl SourceWatch for NativeWatch {}
fn relevant(layout: Layout, roots: &[PathBuf], event: &Event) -> bool {
    if event.need_rescan() {
        return true;
    }
    if matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
    ) {
        return false;
    }
    if matches!(event.kind, EventKind::Any | EventKind::Other) {
        return true;
    }
    event.paths.iter().any(|p| {
        roots.iter().any(|root| {
            if p == root || (p.starts_with(root) && p.extension().is_none()) {
                return true;
            }
            directories(layout, root).iter().any(|d| p.starts_with(d)) && accepted(layout, p)
        })
    })
}
pub(crate) fn watch(
    layout: Layout,
    roots: Vec<PathBuf>,
    changed: ChangeCallback,
) -> Result<Box<dyn SourceWatch>, CoreError> {
    let filter = roots.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if event
            .as_ref()
            .map_or(true, |event| relevant(layout, &filter, event))
        {
            changed();
        }
    })
    .map_err(|_| CoreError::SourceNotDetected)?;
    // Observe roots for later-created input directories; unrelated files/accesses are filtered.
    for root in roots {
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|_| CoreError::SourceNotDetected)?;
    }
    Ok(Box::new(NativeWatch { _watcher: watcher }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_detects_history_rename_truncation_and_scope() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("projects")).unwrap();
        let path = dir.path().join("projects/history.jsonl");
        fs::write(&path, "old").unwrap();
        let read = || {
            inspect(
                Layout::Claude,
                &[dir.path().into()],
                "dataset",
                "UTC",
                "v1",
                &CancellationToken::default(),
            )
            .unwrap()
        };
        let before = read();
        fs::write(&path, "appended").unwrap();
        assert_ne!(before, read());
        let before = read();
        fs::write(&path, "x").unwrap();
        assert_ne!(before, read());
        let before = read();
        fs::rename(&path, path.with_file_name("renamed.jsonl")).unwrap();
        assert_ne!(before, read());
        let before = read();
        fs::remove_file(path.with_file_name("renamed.jsonl")).unwrap();
        assert_ne!(before, read());
        assert_ne!(
            before.scope,
            inspect(
                Layout::Claude,
                &[dir.path().into()],
                "dataset",
                "America/Phoenix",
                "v1",
                &CancellationToken::default()
            )
            .unwrap()
            .scope
        );
        fs::write(dir.path().join("auth.json"), "private").unwrap();
        let before = read();
        fs::write(dir.path().join("auth.json"), "changed").unwrap();
        assert_eq!(before, read());
    }
    #[test]
    fn wal_changes_are_inputs_but_shm_is_not() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("history.db"), "db").unwrap();
        let layout = Layout::Agent(AgentKind::Antigravity);
        let read = || {
            inspect(
                layout,
                &[dir.path().into()],
                "dataset",
                "UTC",
                "v1",
                &CancellationToken::default(),
            )
            .unwrap()
        };
        let before = read();
        fs::write(dir.path().join("history.db-wal"), "wal").unwrap();
        assert_ne!(before, read());
        let before = read();
        fs::write(dir.path().join("history.db-shm"), "lock").unwrap();
        assert_eq!(before, read());
        let mut event = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )));
        event.paths.push(dir.path().join("history.db-wal"));
        assert!(relevant(layout, &[dir.path().into()], &event));
        event.kind = EventKind::Access(notify::event::AccessKind::Read);
        assert!(!relevant(layout, &[dir.path().into()], &event));
    }
    #[test]
    fn replacement_with_identical_metadata_uses_native_identity_and_cancel_stops_checks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        fs::create_dir(dir.path().join("archived_sessions")).unwrap();
        let path = dir.path().join("archived_sessions/history.jsonl");
        fs::write(&path, "old").unwrap();
        let layout = Layout::Agent(AgentKind::Codex);
        let cancel = CancellationToken::default();
        let read = || {
            inspect(
                layout,
                &[dir.path().into()],
                "dataset",
                "UTC",
                "v1",
                &cancel,
            )
            .unwrap()
        };
        let before = read();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        // Keep the old inode allocated, so no filesystem can reuse its identity immediately.
        fs::rename(&path, path.with_extension("saved")).unwrap();
        fs::write(&path, "new").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_ne!(before, read());
        cancel.cancel();
        assert_eq!(
            inspect(
                layout,
                &[dir.path().into()],
                "dataset",
                "UTC",
                "v1",
                &cancel
            ),
            Err(CoreError::Cancelled)
        );
    }
    #[test]
    fn scoped_native_watcher_observes_real_writes_and_drops_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("projects")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = watch(
            Layout::Claude,
            vec![dir.path().into()],
            std::sync::Arc::new(move || {
                let _ = tx.send(());
            }),
        )
        .unwrap();
        fs::write(dir.path().join("projects/synthetic.jsonl"), "synthetic").unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("native content event");
        drop(watcher);
        // No private logs, auth or real minute delays required on either native CI platform.
    }
}
