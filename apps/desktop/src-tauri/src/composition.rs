//! The only place where concrete adapters are selected. Core receives ports, never a Tauri handle.
use crate::{profile::DesktopProfile, settings::SettingsStore};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use usage_adapters::{AgentAdapter, AgentKind, ClaudeCodeAdapter, ProcessRunner, SqliteRepository};
use usage_core::{Clock, CoreError, SourceConfig, UsageService};
pub struct SystemClock;
type Composition = (
    Arc<UsageService>,
    BTreeMap<String, SourceConfig>,
    Arc<SettingsStore>,
);
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
pub fn bootstrap(app_data_dir: &Path, executable_dir: &Path) -> Result<Composition, CoreError> {
    assemble(app_data_dir, &sidecar_path(executable_dir)?)
}
fn assemble(app_data_dir: &Path, executable: &Path) -> Result<Composition, CoreError> {
    let profile = DesktopProfile::load_or_create(app_data_dir)?;
    let settings = Arc::new(SettingsStore::new(app_data_dir, profile.clone()));
    let repository = Arc::new(SqliteRepository::open(app_data_dir.join("usage.db"))?);
    let runner = ProcessRunner::new(executable, Default::default())?;
    let codex = Arc::new(AgentAdapter::new(
        AgentKind::Codex,
        runner.clone(),
        profile.provider("ccusage.codex")?.dataset_id,
        profile.device_id.clone(),
    )?);
    let antigravity = Arc::new(AgentAdapter::new(
        AgentKind::Antigravity,
        runner.clone(),
        profile.provider("ccusage.antigravity")?.dataset_id,
        profile.device_id.clone(),
    )?);
    let configs = profile.configs();
    let source = Arc::new(ClaudeCodeAdapter::new(
        runner,
        profile.claude_dataset_id,
        profile.device_id,
        None,
    )?);
    Ok((
        Arc::new(UsageService::new(
            repository,
            Arc::new(SystemClock),
            vec![codex, antigravity, source],
        )),
        configs,
        settings,
    ))
}
fn sidecar_path(executable_dir: &Path) -> Result<PathBuf, CoreError> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let (filename, resource) = ("ccusage.exe", "ccusage-x86_64-pc-windows-msvc.exe");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let (filename, resource) = ("ccusage", "ccusage-aarch64-apple-darwin");
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    )))]
    {
        let _ = executable_dir;
        return Err(CoreError::SchemaUnsupported);
    }
    #[cfg(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    ))]
    {
        let installed = executable_dir.join(filename);
        if installed.is_file() {
            return Ok(installed);
        }
        // Debug-only development path. Release never executes a workspace/download-cache binary.
        #[cfg(debug_assertions)]
        {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join(resource);
            if source.is_file() {
                return Ok(source);
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = resource;
        Err(CoreError::SourceNotDetected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires target-native pinned sidecar; automatic/manual equivalence on synthetic inputs"]
    async fn automatic_scheduler_matches_manual_full_snapshot_and_stops_on_exit() {
        let binary = std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare pinned sidecar first");
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("fixture logs/projects/synthetic");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("session-a.jsonl"),
            include_bytes!(
                "../../../../tests/fixtures/claude-code/logs/projects/synthetic/session-a.jsonl"
            ),
        )
        .unwrap();
        let mut profile = DesktopProfile::load_or_create(dir.path()).unwrap();
        profile.claude_enabled = true;
        profile.timezone = "America/Phoenix".into();
        profile.claude_root_path = Some(dir.path().join("fixture logs").to_string_lossy().into());
        profile.auto_collection.enabled = true;
        profile.auto_collection.interval_minutes = 1;
        profile.persist(dir.path()).unwrap();
        let (service, configs, settings) = assemble(dir.path(), Path::new(&binary)).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let runtime = crate::runtime::Runtime::new_with_settings(
            service,
            configs,
            Arc::new(move |scan| {
                let _ = tx.send(scan);
            }),
            Some(settings),
        );
        let scheduler = crate::scheduler::Scheduler::new(Arc::new(|_| {}));
        runtime.scheduler.set(scheduler.clone()).ok().unwrap();
        scheduler.start(&runtime).await;
        let scan = tokio::time::timeout(std::time::Duration::from_secs(90), async {
            loop {
                let scan = rx.recv().await.unwrap();
                if matches!(
                    scan.state,
                    usage_contracts::ScanState::Succeeded | usage_contracts::ScanState::Failed
                ) {
                    break scan;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(scan.state, usage_contracts::ScanState::Succeeded);
        runtime.wait_for_idle().await;
        let before = runtime
            .service
            .repository
            .load_snapshots(usage_core::SnapshotFilter::default())
            .await
            .unwrap();
        let manual = runtime
            .start_scan(usage_contracts::StartScanRequest {
                provider_id: "ccusage.claude-code".into(),
                timezone: profile.timezone.clone(),
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(90), async {
            while !runtime
                .get_scan(&manual.job_id)
                .await
                .unwrap()
                .state
                .is_terminal()
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        runtime.wait_for_idle().await;
        let after = runtime
            .service
            .repository
            .load_snapshots(usage_core::SnapshotFilter::default())
            .await
            .unwrap();
        assert_eq!(before.len(), after.len());
        for (automatic, manual) in before.iter().zip(&after) {
            assert_eq!(automatic.key, manual.key);
            assert_eq!(automatic.rows, manual.rows);
            assert_eq!(manual.revision, automatic.revision + 1);
        }
        runtime.shutdown().await;
        assert!(runtime.is_exit_ready());
    }
    #[tokio::test]
    #[ignore = "requires target-native pinned sidecar; uses only copied synthetic logs"]
    async fn composition_to_runtime_maps_ipc_dtos_and_persists_three_scans() {
        let binary = std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare pinned sidecar first");
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("fixture logs/projects/synthetic");
        std::fs::create_dir_all(&logs).unwrap();
        for (name, bytes) in [("session-a.jsonl", include_bytes!("../../../../tests/fixtures/claude-code/logs/projects/synthetic/session-a.jsonl").as_slice()), ("session-b.jsonl", include_bytes!("../../../../tests/fixtures/claude-code/logs/projects/synthetic/session-b.jsonl").as_slice())] {
            std::fs::write(logs.join(name), bytes).unwrap();
        }
        let mut profile = DesktopProfile::load_or_create(dir.path()).unwrap();
        profile.claude_enabled = true;
        profile.claude_root_path = Some(dir.path().join("fixture logs").to_string_lossy().into());
        std::fs::write(
            dir.path().join("profile.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        let (service, configs, settings) = assemble(dir.path(), Path::new(&binary)).unwrap();
        let runtime = crate::runtime::Runtime::new_with_settings(
            service,
            configs,
            Arc::new(|_| {}),
            Some(settings),
        );
        for _ in 0..3 {
            let job = runtime
                .start_scan(usage_contracts::StartScanRequest {
                    provider_id: "ccusage.claude-code".into(),
                    timezone: "America/Phoenix".into(),
                })
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(90), async {
                loop {
                    let scan = runtime.get_scan(&job.job_id).await.unwrap();
                    if scan.state.is_terminal() {
                        assert_eq!(scan.state, usage_core::ScanState::Succeeded);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            // The next click must wait until lifecycle cleanup has released the provider slot.
            runtime.wait_for_idle().await;
        }
        let query = usage_contracts::OverviewQuery {
            range: usage_contracts::DateRange {
                start: "2026-10-03".into(),
                end: "2026-10-05".into(),
            },
            timezone: "America/Phoenix".into(),
            provider_ids: vec![],
            model_ids: vec![],
            bucket: usage_contracts::Bucket::Day,
        };
        let overview = runtime
            .service
            .get_overview(crate::mapping::overview_query(&query).unwrap())
            .await
            .unwrap();
        let dto = crate::mapping::overview(overview, query);
        assert_eq!(dto.usage.tokens.total.value.as_deref(), Some("515"));
        assert!(dto.usage.tokens.output_reasoning.value.is_none());
        runtime.shutdown().await;
        let (reopened, _, _) = assemble(dir.path(), Path::new(&binary)).unwrap();
        let snapshots = reopened
            .repository
            .load_snapshots(usage_core::SnapshotFilter::default())
            .await
            .unwrap();
        assert!(snapshots.iter().all(
            |snapshot| snapshot.revision == 3 && snapshot.origin_device_id == profile.device_id
        ));
    }
    struct System;
    impl crate::timezone::SystemTimezone for System {
        fn detect(&self) -> Result<String, crate::timezone::DetectionError> {
            Ok("UTC".into())
        }
    }
    async fn daily(runtime: &crate::runtime::Runtime, zone: &str) -> (Vec<String>, Option<String>) {
        let query = usage_contracts::OverviewQuery {
            range: usage_contracts::DateRange {
                start: "2026-10-02".into(),
                end: "2026-10-06".into(),
            },
            timezone: zone.into(),
            provider_ids: vec![],
            model_ids: vec![],
            bucket: usage_contracts::Bucket::Day,
        };
        let core = crate::mapping::overview_query(&query).unwrap();
        let overview = runtime.service.get_overview(core).await.unwrap();
        let result = crate::mapping::overview(overview, query);
        let mut days = vec![];
        for bucket in result.buckets {
            let total = bucket.usage.tokens.total.value.unwrap_or_default();
            if !total.is_empty() && total != "0" {
                days.push(bucket.start);
            }
        }
        (days, result.usage.tokens.total.value)
    }
    #[tokio::test]
    #[ignore = "requires target-native pinned sidecar; rebuilds synthetic logs after a zone change"]
    async fn following_a_new_system_zone_rebuilds_daily_buckets_from_source_logs() {
        let binary = std::env::var_os("CCUSAGE_TEST_BINARY").expect("prepare pinned sidecar first");
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("fixture logs/projects/synthetic");
        std::fs::create_dir_all(&logs).unwrap();
        // 06:59Z and 07:01Z fall on both sides of Phoenix midnight but on one UTC day.
        std::fs::write(
            logs.join("session-a.jsonl"),
            include_bytes!(
                "../../../../tests/fixtures/claude-code/logs/projects/synthetic/session-a.jsonl"
            ),
        )
        .unwrap();
        let mut profile = DesktopProfile::load_or_create(dir.path()).unwrap();
        profile.claude_enabled = true;
        profile.timezone = "America/Phoenix".into();
        profile.timezone_mode = crate::profile::TimezoneMode::FollowSystem;
        profile.claude_root_path = Some(dir.path().join("fixture logs").to_string_lossy().into());
        profile.persist(dir.path()).unwrap();
        let (service, configs, settings) = assemble(dir.path(), Path::new(&binary)).unwrap();
        let runtime = crate::runtime::Runtime::new_with_settings(
            service,
            configs,
            Arc::new(|_| {}),
            Some(settings),
        );
        let manual = runtime
            .start_scan(usage_contracts::StartScanRequest {
                provider_id: "ccusage.claude-code".into(),
                timezone: "America/Phoenix".into(),
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(90), async {
            while !runtime
                .get_scan(&manual.job_id)
                .await
                .unwrap()
                .state
                .is_terminal()
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        runtime.wait_for_idle().await;
        // Missing new-zone data stays unavailable; nothing is relabelled from Phoenix days.
        assert_eq!(daily(&runtime, "UTC").await.1, None);
        let monitor = crate::timezone::TimezoneMonitor::new(Arc::new(System), Arc::new(|_| {}));
        runtime.timezone.set(monitor).ok().unwrap();
        runtime.sync_timezone().await;
        tokio::time::timeout(std::time::Duration::from_secs(90), async {
            loop {
                runtime.drive_timezone().await;
                let status = runtime.get_timezone().await.unwrap();
                if status.rebuild == usage_contracts::TimezoneRebuildState::Idle {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let (phoenix, phoenix_total) = daily(&runtime, "America/Phoenix").await;
        let (utc, utc_total) = daily(&runtime, "UTC").await;
        assert_eq!(phoenix, ["2026-10-03", "2026-10-04"]);
        assert_eq!(utc, ["2026-10-04"]);
        assert!(utc_total.is_some());
        assert_eq!(utc_total, phoenix_total);
        let stored = DesktopProfile::load_or_create(dir.path()).unwrap();
        assert_eq!(stored.timezone, "UTC");
        assert!(!stored.timezone_needs_rescan);
        runtime.shutdown().await;
        assert!(runtime.is_exit_ready());
    }
}
