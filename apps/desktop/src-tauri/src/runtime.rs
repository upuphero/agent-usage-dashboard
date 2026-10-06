use crate::{mapping, settings::SettingsStore};
use futures_util::FutureExt;
use std::{
    collections::{BTreeMap, VecDeque},
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use usage_contracts as api;
use usage_core::*;

type ScanEmitter = Arc<dyn Fn(api::ScanSummary) + Send + Sync>;
#[derive(Clone)]
struct ActiveJob {
    id: String,
    cancellation: CancellationToken,
    timezone: String,
    queued: ScanRecord,
    automatic: bool,
}
#[derive(Default)]
struct TerminalCache {
    records: BTreeMap<String, ScanRecord>,
    order: VecDeque<String>,
}
impl TerminalCache {
    fn insert(&mut self, scan: ScanRecord) {
        if !self.records.contains_key(&scan.job_id) {
            self.order.push_back(scan.job_id.clone());
        }
        self.records.insert(scan.job_id.clone(), scan);
        while self.order.len() > 128 {
            if let Some(id) = self.order.pop_front() {
                self.records.remove(&id);
            }
        }
    }
}
pub struct Runtime {
    pub service: Arc<UsageService>,
    pub configs: RwLock<BTreeMap<String, SourceConfig>>,
    settings: Option<Arc<SettingsStore>>,
    active: Mutex<BTreeMap<String, ActiveJob>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    terminal: Mutex<TerminalCache>,
    closing: AtomicBool,
    exit_ready: AtomicBool,
    emit: ScanEmitter,
    pub scheduler: OnceLock<Arc<crate::scheduler::Scheduler>>,
}
impl Runtime {
    #[cfg(test)]
    pub fn new(
        service: Arc<UsageService>,
        configs: BTreeMap<String, SourceConfig>,
        emit: ScanEmitter,
    ) -> Arc<Self> {
        Self::new_with_settings(service, configs, emit, None)
    }
    pub fn new_with_settings(
        service: Arc<UsageService>,
        configs: BTreeMap<String, SourceConfig>,
        emit: ScanEmitter,
        settings: Option<Arc<SettingsStore>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            configs: RwLock::new(configs),
            settings,
            active: Mutex::new(BTreeMap::new()),
            tasks: Mutex::new(vec![]),
            terminal: Mutex::new(TerminalCache::default()),
            closing: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
            emit,
            scheduler: OnceLock::new(),
        })
    }
    pub async fn start_scan(
        self: &Arc<Self>,
        request: api::StartScanRequest,
    ) -> Result<api::StartScanResult, CoreError> {
        self.start_scan_inner(request, None)
            .await?
            .ok_or(CoreError::ScanBusy)
    }
    pub async fn start_automatic_scan(
        self: &Arc<Self>,
        provider: &str,
        timezone: &str,
        config: &SourceConfig,
    ) -> Result<Option<api::StartScanResult>, CoreError> {
        self.start_scan_inner(
            api::StartScanRequest {
                provider_id: provider.into(),
                timezone: timezone.into(),
            },
            Some(config),
        )
        .await
    }
    async fn start_scan_inner(
        self: &Arc<Self>,
        request: api::StartScanRequest,
        automatic: Option<&SourceConfig>,
    ) -> Result<Option<api::StartScanResult>, CoreError> {
        application::validate_timezone(&request.timezone)?;
        self.service.source(&request.provider_id)?;
        let mut active = self.active.lock().await;
        if self.closing.load(Ordering::Acquire) {
            return Err(CoreError::ShuttingDown);
        }
        let config = self
            .configs
            .read()
            .await
            .get(&request.provider_id)
            .cloned()
            .unwrap_or_default();
        if !config.enabled {
            return Err(CoreError::ProviderDisabled);
        }
        if let Some(expected) = automatic {
            let settings = self.settings.as_ref().ok_or(CoreError::UnsupportedFilter)?;
            let (_, timezone, auto) = settings.auto_config().await;
            if !auto.enabled
                || timezone != request.timezone
                || expected.root_path != config.root_path
                || !active.is_empty()
            {
                return Ok(None);
            }
        }
        if let Some(job) = active.get_mut(&request.provider_id) {
            if job.timezone != request.timezone {
                return Err(CoreError::ScanBusy);
            }
            // A user's joined task must survive turning automatic collection off.
            job.automatic = false;
            return Ok(Some(api::StartScanResult {
                job_id: job.id.clone(),
            }));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancellation = CancellationToken::default();
        let queued = ScanRecord {
            job_id: id.clone(),
            provider_id: request.provider_id.clone(),
            state: ScanState::Queued,
            started_at: self.service.clock.now(),
            finished_at: None,
            error: None,
            snapshots_replaced: 0,
            rows_written: 0,
        };
        self.service.repository.save_scan(queued.clone()).await?;
        (self.emit)(mapping::scan(queued.clone()));
        active.insert(
            request.provider_id.clone(),
            ActiveJob {
                id: id.clone(),
                cancellation: cancellation.clone(),
                timezone: request.timezone.clone(),
                queued: queued.clone(),
                automatic: automatic.is_some(),
            },
        );
        let runtime = self.clone();
        let job_id = id.clone();
        let task = tokio::spawn(async move {
            let outcome = AssertUnwindSafe(runtime.service.run_scan(
                job_id.clone(),
                request.provider_id.clone(),
                CollectRequest {
                    timezone: request.timezone.clone(),
                    config,
                },
                cancellation,
            ))
            .catch_unwind()
            .await;
            match outcome {
                Ok(Ok(record)) => {
                    let succeeded = record.state == ScanState::Succeeded;
                    runtime.publish_terminal(record, false).await;
                    if succeeded {
                        if let Some(settings) = &runtime.settings {
                            // Keep the persisted retry marker if the rebuild or its acknowledgement fails.
                            let _ = settings
                                .complete_timezone_rescan(&request.provider_id, &request.timezone)
                                .await;
                        }
                    }
                }
                failure => {
                    let mut failed = queued;
                    failed.state = ScanState::Failed;
                    failed.finished_at = Some(runtime.service.clock.now());
                    failed.error = Some(match failure {
                        Ok(Err(error)) => error,
                        _ => CoreError::CollectionFailed,
                    });
                    runtime.publish_terminal(failed, true).await;
                }
            }
            runtime.active.lock().await.remove(&request.provider_id);
        });
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
        Ok(Some(api::StartScanResult { job_id: id }))
    }
    pub async fn get_scan(&self, id: &str) -> Result<ScanRecord, CoreError> {
        if id.is_empty() || id.len() > 128 {
            return Err(CoreError::InvalidQuery);
        }
        if let Some(scan) = self.terminal.lock().await.records.get(id).cloned() {
            return Ok(scan);
        }
        self.service
            .repository
            .get_scan(id)
            .await?
            .ok_or(CoreError::ScanNotFound)
    }
    async fn publish_terminal(&self, scan: ScanRecord, persist: bool) {
        let _ = self.service.observe_scan(scan.clone());
        self.terminal.lock().await.insert(scan.clone());
        if persist {
            // Diagnostics contain no raw exception/process text. Cache remains available if storage is busy.
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                self.service.repository.save_scan(scan.clone()),
            )
            .await;
        }
        (self.emit)(mapping::scan(scan));
    }
    pub async fn list_providers(&self) -> Result<Vec<ProviderStatus>, CoreError> {
        let configs = self.configs.read().await.clone();
        let mut statuses = self.service.list_providers(&configs).await?;
        let cache = self.terminal.lock().await;
        for status in &mut statuses {
            if let Some(terminal) = status
                .last_scan
                .as_ref()
                .and_then(|scan| cache.records.get(&scan.job_id))
            {
                application::apply_provider_scan(status, Some(terminal.clone()));
            }
        }
        Ok(statuses)
    }
    /// Called once by composition before commands are accepted. Interrupted work is never resumed implicitly.
    pub async fn recover_interrupted_scans(&self) -> Result<(), CoreError> {
        if !self.active.lock().await.is_empty() {
            return Err(CoreError::ScanBusy);
        }
        for source in &self.service.sources {
            for mut scan in self
                .service
                .repository
                .list_scans(&source.descriptor().provider_id)
                .await?
            {
                if !scan.state.is_terminal() {
                    scan.state = ScanState::Cancelled;
                    scan.finished_at = Some(self.service.clock.now());
                    scan.error = Some(CoreError::Cancelled);
                    self.publish_terminal(scan, true).await;
                }
            }
        }
        Ok(())
    }
    /// Rebuild daily buckets from raw logs after correcting the legacy UTC default.
    pub async fn rescan_changed_timezone(self: &Arc<Self>) -> Result<(), CoreError> {
        let Some(settings) = &self.settings else {
            return Ok(());
        };
        let providers: Vec<_> = self
            .configs
            .read()
            .await
            .iter()
            .filter(|(_, config)| config.enabled)
            .map(|(provider, _)| provider.clone())
            .collect();
        let Some(timezone) = settings.take_timezone_rescan(&providers).await else {
            return Ok(());
        };
        if providers.is_empty() {
            return settings.complete_timezone_rescan("", &timezone).await;
        }
        for provider_id in providers {
            self.start_scan(api::StartScanRequest {
                provider_id,
                timezone: timezone.clone(),
            })
            .await?;
        }
        Ok(())
    }
    pub async fn cancel_scan(&self, id: &str) -> Result<(), CoreError> {
        let scan = self.get_scan(id).await?;
        if scan.state.is_terminal() {
            return Ok(());
        }
        if let Some(job) = self.active.lock().await.values().find(|job| job.id == id) {
            job.cancellation.cancel();
        }
        Ok(())
    }
    pub fn settings_available(&self) -> bool {
        self.settings.is_some()
    }
    pub async fn get_settings(&self) -> Result<api::SettingsResult, api::ApiError> {
        Ok(self
            .settings
            .as_ref()
            .ok_or_else(|| mapping::error(CoreError::UnsupportedFilter))?
            .get()
            .await)
    }
    pub async fn update_settings(
        &self,
        request: api::UpdateSettingsRequest,
    ) -> Result<api::SettingsResult, api::ApiError> {
        // Hold this guard through persistence and config swap; a scan cannot capture stale configuration.
        let active = self.active.lock().await;
        if self.closing.load(Ordering::Acquire) {
            return Err(mapping::error(CoreError::ShuttingDown));
        }
        if !active.is_empty() {
            return Err(mapping::error(CoreError::ScanBusy));
        }
        let store = self
            .settings
            .as_ref()
            .ok_or_else(|| mapping::error(CoreError::UnsupportedFilter))?;
        let (result, configs) = store.update(request).await?;
        *self.configs.write().await = configs;
        if let Some(scheduler) = self.scheduler.get() {
            scheduler.reconfigure(true);
        }
        Ok(result)
    }
    pub async fn auto_context(
        &self,
    ) -> Option<(
        String,
        String,
        api::AutoCollectionConfig,
        BTreeMap<String, SourceConfig>,
    )> {
        let _active = self.active.lock().await;
        let (revision, timezone, config) = self.settings.as_ref()?.auto_config().await;
        Some((
            revision,
            timezone,
            config,
            self.configs.read().await.clone(),
        ))
    }
    pub async fn has_active_jobs(&self) -> bool {
        !self.active.lock().await.is_empty()
    }
    pub async fn get_auto_collection(&self) -> Result<api::AutoCollectionStatus, api::ApiError> {
        let (revision, timezone, config, configs) = self
            .auto_context()
            .await
            .ok_or_else(|| mapping::error(CoreError::UnsupportedFilter))?;
        let mut status = match self.scheduler.get() {
            Some(scheduler) => scheduler.status().await,
            None => None,
        }
        .unwrap_or(api::AutoCollectionStatus {
            api_version: api::API_VERSION.into(),
            revision: revision.clone(),
            config: config.clone(),
            timezone: timezone.clone(),
            providers: vec![],
        });
        status.revision = revision;
        status.timezone = timezone;
        status.config = config;
        status.providers.retain(|p| {
            status.config.enabled && configs.get(&p.provider_id).is_some_and(|c| c.enabled)
        });
        if status.config.enabled {
            for (provider, _) in configs.iter().filter(|(_, c)| c.enabled) {
                if !status.providers.iter().any(|p| &p.provider_id == provider) {
                    status.providers.push(api::AutoProviderStatus {
                        provider_id: provider.clone(),
                        state: api::AutoCollectionState::Waiting,
                        job_id: None,
                        last_success_at: None,
                        next_check_at: None,
                        watching: false,
                        error: None,
                    });
                }
            }
        }
        Ok(status)
    }
    pub async fn update_auto_collection(
        &self,
        request: api::UpdateAutoCollectionRequest,
    ) -> Result<api::AutoCollectionStatus, api::ApiError> {
        {
            let active = self.active.lock().await;
            if self.closing.load(Ordering::Acquire) {
                return Err(mapping::error(CoreError::ShuttingDown));
            }
            let store = self
                .settings
                .as_ref()
                .ok_or_else(|| mapping::error(CoreError::UnsupportedFilter))?;
            let (_, _, previous) = store.auto_config().await;
            let enabled = request.config.enabled;
            store.update_auto(request).await?;
            if !enabled {
                for job in active.values().filter(|job| job.automatic) {
                    job.cancellation.cancel();
                }
            }
            if let Some(scheduler) = self.scheduler.get() {
                scheduler.reconfigure(previous.enabled != enabled);
            }
        }
        if let Some(scheduler) = self.scheduler.get() {
            scheduler.wake();
        }
        self.get_auto_collection().await
    }
    pub async fn remember_directory(
        &self,
        provider_id: &str,
        path: std::path::PathBuf,
    ) -> Result<api::SourceDirectory, api::ApiError> {
        self.service.source(provider_id).map_err(mapping::error)?;
        self.settings
            .as_ref()
            .ok_or_else(|| mapping::error(CoreError::UnsupportedFilter))?
            .remember_directory_for(provider_id, path)
            .await
    }
    /// Caller prevents window close until children have observed cancellation and completed.
    pub async fn shutdown(&self) {
        self.shutdown_with_grace(Duration::from_secs(10)).await;
    }
    async fn shutdown_with_grace(&self, grace: Duration) {
        self.closing.store(true, Ordering::Release);
        if let Some(scheduler) = self.scheduler.get() {
            scheduler.stop().await;
        }
        for job in self.active.lock().await.values() {
            job.cancellation.cancel();
        }
        let tasks = std::mem::take(&mut *self.tasks.lock().await);
        let deadline = tokio::time::Instant::now() + grace;
        for mut task in tasks {
            if tokio::time::timeout_at(deadline, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
        let remaining = std::mem::take(&mut *self.active.lock().await);
        for job in remaining.into_values() {
            if self.terminal.lock().await.records.contains_key(&job.id) {
                continue;
            }
            // A completed transaction may have won before the task was stopped. Preserve a stored terminal state.
            let stored = tokio::time::timeout(
                Duration::from_secs(2),
                self.service.repository.get_scan(&job.id),
            )
            .await;
            if let Ok(Ok(Some(scan))) = stored {
                if scan.state.is_terminal() {
                    self.publish_terminal(scan, false).await;
                    continue;
                }
            }
            let mut scan = job.queued;
            scan.state = if job.cancellation.commit_started() {
                ScanState::Failed
            } else {
                ScanState::Cancelled
            };
            scan.finished_at = Some(self.service.clock.now());
            scan.error = Some(if job.cancellation.commit_started() {
                CoreError::Storage
            } else {
                CoreError::Cancelled
            });
            self.publish_terminal(scan, true).await;
        }
        self.exit_ready.store(true, Ordering::Release);
    }
    pub fn begin_shutdown(&self) -> bool {
        if let Some(scheduler) = self.scheduler.get() {
            scheduler.begin_stop();
        }
        self.closing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn is_exit_ready(&self) -> bool {
        self.exit_ready.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub async fn wait_for_idle(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.active.lock().await.is_empty() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("provider lifecycle cleanup timed out");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usage_core::memory::{FixedClock, MemoryRepository};
    struct Source;
    #[async_trait::async_trait]
    impl UsageSource for Source {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                provider_id: "fixture".into(),
                product_id: "claude-code".into(),
                display_name: "Fixture".into(),
                capabilities: ProviderCapabilities {
                    report_kinds: vec![ReportKind::Daily],
                    supported_dimensions: vec!["day".into()],
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
            loop {
                cancellation.check()?;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    fn runtime() -> Arc<Runtime> {
        let service = Arc::new(UsageService::new(
            Arc::new(MemoryRepository::default()),
            Arc::new(FixedClock(
                chrono::DateTime::parse_from_rfc3339("2026-10-04T12:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )),
            vec![Arc::new(Source)],
        ));
        Runtime::new(
            service,
            BTreeMap::from([(
                "fixture".into(),
                SourceConfig {
                    enabled: true,
                    root_path: None,
                },
            )]),
            Arc::new(|_| {}),
        )
    }
    fn request(zone: &str) -> api::StartScanRequest {
        api::StartScanRequest {
            provider_id: "fixture".into(),
            timezone: zone.into(),
        }
    }
    #[tokio::test]
    async fn auto_start_is_atomic_manual_has_priority_and_disable_cancels_only_owned_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = crate::profile::DesktopProfile::load_or_create(dir.path()).unwrap();
        profile.timezone = "America/Phoenix".into();
        profile.auto_collection.enabled = true;
        let store = Arc::new(SettingsStore::new(dir.path(), profile));
        let mut runtime = runtime();
        Arc::get_mut(&mut runtime).unwrap().settings = Some(store);
        let config = SourceConfig {
            enabled: true,
            root_path: None,
        };
        let manual = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        assert!(runtime
            .start_automatic_scan("fixture", "America/Phoenix", &config)
            .await
            .unwrap()
            .is_none());
        runtime
            .update_auto_collection(api::UpdateAutoCollectionRequest {
                expected_revision: "1".into(),
                config: api::AutoCollectionConfig {
                    enabled: false,
                    interval_minutes: 1,
                },
            })
            .await
            .unwrap();
        assert!(!runtime
            .active
            .lock()
            .await
            .values()
            .next()
            .unwrap()
            .cancellation
            .is_cancelled());
        assert_eq!(
            runtime
                .update_settings(api::UpdateSettingsRequest {
                    expected_revision: "2".into(),
                    timezone: "UTC".into(),
                    providers: vec![]
                })
                .await
                .unwrap_err()
                .code,
            api::ErrorCode::ScanBusy
        );
        runtime.cancel_scan(&manual.job_id).await.unwrap();
        runtime.wait_for_idle().await;
        runtime
            .update_auto_collection(api::UpdateAutoCollectionRequest {
                expected_revision: "2".into(),
                config: api::AutoCollectionConfig {
                    enabled: true,
                    interval_minutes: 1,
                },
            })
            .await
            .unwrap();
        let automatic = runtime
            .start_automatic_scan("fixture", "America/Phoenix", &config)
            .await
            .unwrap()
            .unwrap();
        runtime
            .update_auto_collection(api::UpdateAutoCollectionRequest {
                expected_revision: "3".into(),
                config: api::AutoCollectionConfig {
                    enabled: false,
                    interval_minutes: 1,
                },
            })
            .await
            .unwrap();
        runtime.wait_for_idle().await;
        assert_eq!(
            runtime.get_scan(&automatic.job_id).await.unwrap().state,
            ScanState::Cancelled
        );
        assert!(runtime
            .start_automatic_scan("fixture", "America/Phoenix", &config)
            .await
            .unwrap()
            .is_none());
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn duplicate_clicks_merge_timezone_change_is_busy_and_shutdown_cancels() {
        let runtime = runtime();
        let first = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        let duplicate = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        assert_eq!(first.job_id, duplicate.job_id);
        assert_eq!(
            runtime.start_scan(request("UTC")).await.unwrap_err(),
            CoreError::ScanBusy
        );
        runtime.cancel_scan(&first.job_id).await.unwrap();
        runtime.shutdown().await;
        assert_eq!(
            runtime.get_scan(&first.job_id).await.unwrap().state,
            ScanState::Cancelled
        );
        assert_eq!(
            runtime
                .start_scan(request("America/Phoenix"))
                .await
                .unwrap_err(),
            CoreError::ShuttingDown
        );
        runtime.cancel_scan(&first.job_id).await.unwrap();
    }
    #[tokio::test]
    async fn unknown_jobs_return_stable_error() {
        assert_eq!(
            runtime().cancel_scan("unknown").await.unwrap_err(),
            CoreError::ScanNotFound
        );
    }
    #[tokio::test]
    async fn timezone_migration_scans_only_enabled_sources_once_in_the_new_zone() {
        let base = runtime();
        let directory = tempfile::tempdir().unwrap();
        let mut profile = crate::profile::DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone = "America/Phoenix".into();
        profile.timezone_needs_rescan = true;
        let mut configs = base.configs.read().await.clone();
        configs.insert("disabled".into(), SourceConfig::default());
        let runtime = Runtime::new_with_settings(
            base.service.clone(),
            configs,
            Arc::new(|_| {}),
            Some(Arc::new(SettingsStore::new(directory.path(), profile))),
        );
        runtime.rescan_changed_timezone().await.unwrap();
        let first = runtime.active.lock().await["fixture"].clone();
        assert_eq!(first.timezone, "America/Phoenix");
        assert_eq!(runtime.active.lock().await.len(), 1);
        runtime.rescan_changed_timezone().await.unwrap();
        assert_eq!(runtime.active.lock().await["fixture"].id, first.id);
        runtime.shutdown().await;
        runtime.rescan_changed_timezone().await.unwrap();
        assert!(runtime.active.lock().await.is_empty());
    }

    #[tokio::test]
    async fn startup_recovers_abandoned_jobs_and_keeps_completed_jobs() {
        let runtime = runtime();
        let started_at = runtime.service.clock.now() - chrono::Duration::seconds(5);
        let pending = ScanRecord {
            job_id: "interrupted".into(),
            provider_id: "fixture".into(),
            state: ScanState::Running,
            started_at,
            finished_at: None,
            error: None,
            snapshots_replaced: 0,
            rows_written: 0,
        };
        runtime
            .service
            .repository
            .save_scan(pending.clone())
            .await
            .unwrap();
        let mut complete = pending;
        complete.job_id = "complete".into();
        complete.state = ScanState::Succeeded;
        complete.finished_at = Some(runtime.service.clock.now());
        complete.snapshots_replaced = 1;
        runtime
            .service
            .repository
            .save_scan(complete)
            .await
            .unwrap();
        runtime.recover_interrupted_scans().await.unwrap();
        assert_eq!(
            runtime.get_scan("interrupted").await.unwrap().state,
            ScanState::Cancelled
        );
        assert_eq!(
            runtime.get_scan("interrupted").await.unwrap().started_at,
            started_at
        );
        assert_eq!(
            runtime.get_scan("complete").await.unwrap().state,
            ScanState::Succeeded
        );
    }

    #[test]
    fn terminal_cache_is_bounded_without_evicting_a_repeated_job() {
        let mut cache = TerminalCache::default();
        for i in 0..129 {
            cache.insert(ScanRecord {
                job_id: i.to_string(),
                provider_id: "fixture".into(),
                state: ScanState::Cancelled,
                started_at: chrono::DateTime::parse_from_rfc3339("2026-10-04T12:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                finished_at: None,
                error: Some(CoreError::Cancelled),
                snapshots_replaced: 0,
                rows_written: 0,
            });
        }
        assert_eq!(cache.records.len(), 128);
        assert!(!cache.records.contains_key("0"));
        let value = cache.records["128"].clone();
        cache.insert(value);
        assert_eq!(cache.order.len(), 128);
        assert!(cache.records.contains_key("128"));
    }

    struct ExceptionalSource {
        panic: bool,
        entered: Arc<tokio::sync::Notify>,
        dropped: Arc<AtomicBool>,
    }
    struct DropSignal(Arc<AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[async_trait::async_trait]
    impl UsageSource for ExceptionalSource {
        fn descriptor(&self) -> ProviderDescriptor {
            Source.descriptor()
        }
        async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError> {
            Source.detect(config).await
        }
        async fn collect(
            &self,
            _: CollectRequest,
            _: CancellationToken,
        ) -> Result<CollectionBatch, CoreError> {
            let _guard = DropSignal(self.dropped.clone());
            self.entered.notify_one();
            if self.panic {
                panic!("synthetic source failure");
            }
            std::future::pending().await
        }
    }
    fn exceptional(panic: bool) -> (Arc<Runtime>, Arc<tokio::sync::Notify>, Arc<AtomicBool>) {
        let base = runtime();
        let entered = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let source = Arc::new(ExceptionalSource {
            panic,
            entered: entered.clone(),
            dropped: dropped.clone(),
        });
        let service = Arc::new(UsageService::new(
            base.service.repository.clone(),
            base.service.clock.clone(),
            vec![source],
        ));
        (
            Runtime::new(
                service,
                BTreeMap::from([(
                    "fixture".into(),
                    SourceConfig {
                        enabled: true,
                        root_path: None,
                    },
                )]),
                Arc::new(|_| {}),
            ),
            entered,
            dropped,
        )
    }
    #[tokio::test]
    async fn source_panic_becomes_redacted_failure_and_releases_provider() {
        let (runtime, entered, dropped) = exceptional(true);
        let job = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        runtime.wait_for_idle().await;
        let scan = runtime.get_scan(&job.job_id).await.unwrap();
        assert_eq!(scan.state, ScanState::Failed);
        assert_eq!(scan.error, Some(CoreError::CollectionFailed));
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(
            runtime.list_providers().await.unwrap()[0].detection.state,
            ProviderState::Error
        );
    }
    #[tokio::test]
    async fn shutdown_aborts_uncooperative_collection_and_finishes_its_record() {
        let (runtime, entered, dropped) = exceptional(false);
        let job = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        runtime.shutdown_with_grace(Duration::from_millis(20)).await;
        assert!(dropped.load(Ordering::Acquire));
        assert!(runtime.is_exit_ready());
        assert_eq!(
            runtime.get_scan(&job.job_id).await.unwrap().state,
            ScanState::Cancelled
        );
        assert_eq!(
            runtime
                .service
                .repository
                .get_scan(&job.job_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            ScanState::Cancelled
        );
    }
    #[tokio::test]
    async fn settings_write_is_blocked_during_scan_without_changing_revision() {
        let directory = tempfile::tempdir().unwrap();
        let profile = crate::profile::DesktopProfile::load_or_create(directory.path()).unwrap();
        let store = Arc::new(SettingsStore::new(directory.path(), profile));
        let base = runtime();
        let runtime = Runtime::new_with_settings(
            base.service.clone(),
            BTreeMap::from([(
                "fixture".into(),
                SourceConfig {
                    enabled: true,
                    root_path: None,
                },
            )]),
            Arc::new(|_| {}),
            Some(store),
        );
        let job = runtime
            .start_scan(request("America/Phoenix"))
            .await
            .unwrap();
        let result = runtime
            .update_settings(api::UpdateSettingsRequest {
                expected_revision: "1".into(),
                timezone: "UTC".into(),
                providers: vec![],
            })
            .await;
        assert_eq!(result.unwrap_err().code, api::ErrorCode::ScanBusy);
        assert_eq!(runtime.get_settings().await.unwrap().revision, "1");
        runtime.cancel_scan(&job.job_id).await.unwrap();
        runtime.shutdown().await;
    }
}
