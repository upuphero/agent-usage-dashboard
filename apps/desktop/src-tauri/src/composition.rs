//! The only place where concrete adapters are selected. Core receives ports, never a Tauri handle.
use crate::{profile::DesktopProfile, settings::SettingsStore};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use usage_adapters::{ClaudeCodeAdapter, ProcessRunner, SqliteRepository};
use usage_core::{Clock, CoreError, SourceConfig, UsageService};
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
pub fn bootstrap(
    app_data_dir: &Path,
    executable_dir: &Path,
) -> Result<
    (
        Arc<UsageService>,
        BTreeMap<String, SourceConfig>,
        Arc<SettingsStore>,
    ),
    CoreError,
> {
    assemble(app_data_dir, &sidecar_path(executable_dir)?)
}
fn assemble(
    app_data_dir: &Path,
    executable: &Path,
) -> Result<
    (
        Arc<UsageService>,
        BTreeMap<String, SourceConfig>,
        Arc<SettingsStore>,
    ),
    CoreError,
> {
    let profile = DesktopProfile::load_or_create(app_data_dir)?;
    let settings = Arc::new(SettingsStore::new(app_data_dir, profile.clone()));
    let repository = Arc::new(SqliteRepository::open(app_data_dir.join("usage.db"))?);
    let runner = ProcessRunner::new(executable, Default::default())?;
    let source = Arc::new(ClaudeCodeAdapter::new(
        runner,
        profile.claude_dataset_id,
        profile.device_id,
        None,
    )?);
    let configs = BTreeMap::from([(
        "ccusage.claude-code".into(),
        SourceConfig {
            enabled: profile.claude_enabled,
            root_path: profile.claude_root_path,
        },
    )]);
    Ok((
        Arc::new(UsageService::new(
            repository,
            Arc::new(SystemClock),
            vec![source],
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
}
