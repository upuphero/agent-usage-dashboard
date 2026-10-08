use crate::{
    mapping,
    profile::TimezoneMode,
    settings::SettingsStore,
    timezone::{DetectionError, TimezoneMonitor},
};
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
    rebuild: bool,
}
/// Automatic and rebuild starts are accepted only while every provider slot is idle.
#[derive(Clone, Copy)]
enum Origin<'a> {
    Manual,
    Automatic(&'a SourceConfig),
    Rebuild,
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
    pub timezone: OnceLock<Arc<TimezoneMonitor>>,
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
            timezone: OnceLock::new(),
        })
    }
    pub async fn start_scan(
        self: &Arc<Self>,
        request: api::StartScanRequest,
    ) -> Result<api::StartScanResult, CoreError> {
        self.start_scan_inner(request, Origin::Manual)
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
            Origin::Automatic(config),
        )
        .await
    }
    async fn start_scan_inner(
        self: &Arc<Self>,
        request: api::StartScanRequest,
        origin: Origin<'_>,
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
        match origin {
            Origin::Manual => {}
            Origin::Automatic(expected) => {
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
            Origin::Rebuild => {
                // Re-check under the gate: a finished source, newer target or busy slot defers it.
                let (provider, zone) = (&request.provider_id, &request.timezone);
                if !active.is_empty() || !self.rebuild_allowed(provider, zone).await {
                    return Ok(None);
                }
            }
        }
        if let Some(job) = active.get_mut(&request.provider_id) {
            if job.timezone != request.timezone {
                return Err(CoreError::ScanBusy);
            }
            // A user's joined task must survive turning automatic collection off.
            job.automatic = false;
            job.rebuild = false;
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
                automatic: matches!(origin, Origin::Automatic(_)),
                rebuild: matches!(origin, Origin::Rebuild),
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
            let (state, error) = match outcome {
                Ok(Ok(record)) => {
                    let finished = (record.state, record.error);
                    runtime.publish_terminal(record, false).await;
                    finished
                }
                failure => {
                    let mut failed = queued;
                    failed.state = ScanState::Failed;
                    failed.finished_at = Some(runtime.service.clock.now());
                    failed.error = Some(match failure {
                        Ok(Err(error)) => error,
                        _ => CoreError::CollectionFailed,
                    });
                    let finished = (failed.state, failed.error);
                    runtime.publish_terminal(failed, true).await;
                    finished
                }
            };
            // Acknowledge before releasing the slot, so the next rebuild step sees this result.
            let rebuild = runtime
                .active
                .lock()
                .await
                .get(&request.provider_id)
                .is_some_and(|job| job.rebuild);
            runtime
                .record_timezone_outcome(&request, state, error, rebuild)
                .await;
            runtime.active.lock().await.remove(&request.provider_id);
            runtime.wake_timezone(false);
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
    pub fn timezone_available(&self) -> bool {
        self.settings.is_some() && self.timezone.get().is_some()
    }
    pub fn wake_timezone(&self, detect: bool) {
        if let Some(monitor) = self.timezone.get() {
            monitor.wake(detect);
        }
    }
    /// Rebuild preconditions; start_scan_inner evaluates them again while holding the gate.
    async fn rebuild_allowed(&self, provider: &str, zone: &str) -> bool {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return false;
        };
        let (_, _, auto) = settings.auto_config().await;
        let target = settings.rebuild_target().await;
        let done = settings.rebuilt().await.contains(provider);
        !auto.enabled && !done && !monitor.rebuild_waiting() && target.as_deref() == Some(zone)
    }
    async fn enabled_providers(&self) -> Vec<String> {
        self.configs
            .read()
            .await
            .iter()
            .filter(|(_, config)| config.enabled)
            .map(|(provider, _)| provider.clone())
            .collect()
    }
    /// Startup check, before the first query can read the effective zone.
    pub async fn sync_timezone(&self) {
        let Some(monitor) = self.timezone.get() else {
            return;
        };
        let detection = monitor.detect().await;
        self.observe_system_timezone(detection).await;
        self.apply_pending_timezone().await;
    }
    /// Records one detection; Follow mode only queues the zone for the protected boundary.
    pub async fn observe_system_timezone(&self, detection: Result<String, DetectionError>) {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return;
        };
        let view = settings.timezone_view().await;
        let follow = view.mode == TimezoneMode::FollowSystem;
        monitor.observe(detection, follow, &view.effective);
    }
    /// Same gate as settings writes: running jobs keep their captured zone until they finish.
    async fn apply_pending_timezone(&self) {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return;
        };
        let Some(target) = monitor.pending() else {
            return;
        };
        let active = self.active.lock().await;
        if self.closing.load(Ordering::Acquire) || !active.is_empty() {
            return;
        }
        // A storage failure keeps the target pending; the next tick retries.
        if let Ok(changed) = settings.follow_system_timezone(&target).await {
            monitor.applied(&target);
            if changed {
                monitor.reset_rebuild();
                if let Some(scheduler) = self.scheduler.get() {
                    scheduler.reconfigure(true);
                }
            }
        }
    }
    /// One native monitor step: apply a safe pending zone, acknowledge completion and start the
    /// next serial rebuild. With Automatic collection on, its fresh scheduler scope rebuilds.
    pub async fn drive_timezone(self: &Arc<Self>) {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return;
        };
        self.apply_pending_timezone().await;
        let enabled = self.enabled_providers().await;
        if let Ok(true) = settings.complete_rebuild_if_done(&enabled).await {
            monitor.reset_rebuild();
        }
        let view = settings.timezone_view().await;
        let (_, _, auto) = settings.auto_config().await;
        if view.needs_rescan && !auto.enabled && monitor.retry_allowed(monitor.now()) {
            let rebuilt = settings.rebuilt().await;
            if let Some(provider) = enabled.into_iter().find(|id| !rebuilt.contains(id)) {
                let request = api::StartScanRequest {
                    provider_id: provider.clone(),
                    timezone: view.effective,
                };
                match self.start_scan_inner(request, Origin::Rebuild).await {
                    // A source disabled since this step began is no longer part of the rebuild.
                    Ok(_) | Err(CoreError::ScanBusy | CoreError::ShuttingDown) => {}
                    Err(CoreError::ProviderDisabled) => {}
                    Err(error) => monitor.record_failure(&provider, error, false, true),
                }
            }
        }
        self.publish_timezone_status().await;
    }
    /// Any job may acknowledge the current target; only rebuild-owned failures move its backoff.
    async fn record_timezone_outcome(
        &self,
        request: &api::StartScanRequest,
        state: ScanState,
        error: Option<CoreError>,
        rebuild: bool,
    ) {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return;
        };
        let (provider, zone) = (request.provider_id.as_str(), request.timezone.as_str());
        if state == ScanState::Succeeded {
            if settings.record_rebuild_success(provider, zone).await {
                monitor.clear_error(provider);
                let enabled = self.enabled_providers().await;
                if let Ok(true) = settings.complete_rebuild_if_done(&enabled).await {
                    monitor.reset_rebuild();
                }
            }
        } else if settings.rebuild_target().await.as_deref() == Some(zone) {
            let error = error.unwrap_or(CoreError::CollectionFailed);
            let cancelled = state == ScanState::Cancelled;
            monitor.record_failure(provider, error, cancelled, rebuild);
        }
    }
    /// Status and its sequence are produced under one lock, so an older snapshot never wins.
    pub async fn publish_timezone_status(&self) {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return;
        };
        let mut published = monitor.status_lock().await;
        let status = self.timezone_status(settings, monitor).await;
        if published.changed(&status) {
            monitor.emit(published.next(status));
        }
    }
    pub async fn get_timezone(&self) -> Result<api::TimezoneStatus, api::ApiError> {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return Err(mapping::error(CoreError::UnsupportedFilter));
        };
        let mut published = monitor.status_lock().await;
        let status = self.timezone_status(settings, monitor).await;
        Ok(published.next(status))
    }
    async fn timezone_status(
        &self,
        settings: &SettingsStore,
        monitor: &TimezoneMonitor,
    ) -> api::TimezoneStatus {
        use api::TimezoneProviderState as Step;
        let view = settings.timezone_view().await;
        // Disabled sources are never part of a rebuild and are never read for it.
        let enabled = if view.needs_rescan {
            self.enabled_providers().await
        } else {
            vec![]
        };
        let rebuilt = settings.rebuilt().await;
        let running = self.active.lock().await.clone();
        let snapshot = monitor.snapshot();
        let mut providers = vec![];
        for id in enabled {
            let job = running.get(&id);
            let (state, job_id, error) = if rebuilt.contains(&id) {
                (Step::Succeeded, None, None)
            } else if let Some(job) = job.filter(|job| job.timezone == view.effective) {
                (Step::Rebuilding, Some(job.id.clone()), None)
            } else if let Some(error) = snapshot.errors.get(&id) {
                (Step::Failed, None, Some(error.clone()))
            } else {
                (Step::Pending, None, None)
            };
            providers.push(api::TimezoneProviderStatus {
                provider_id: id,
                state,
                job_id,
                error,
            });
        }
        let rebuilding = providers.iter().any(|p| p.state == Step::Rebuilding);
        let rebuild = if !view.needs_rescan {
            api::TimezoneRebuildState::Idle
        } else if rebuilding {
            api::TimezoneRebuildState::Rebuilding
        } else if snapshot.retry_at > monitor.now() {
            api::TimezoneRebuildState::Backoff
        } else {
            api::TimezoneRebuildState::Pending
        };
        let backoff = rebuild == api::TimezoneRebuildState::Backoff;
        api::TimezoneStatus {
            api_version: api::API_VERSION.into(),
            sequence: String::new(),
            revision: view.revision,
            mode: view.mode.into(),
            effective_timezone: view.effective,
            system_timezone: snapshot.system,
            detection_error: snapshot.detection_error,
            pending_timezone: snapshot.pending,
            rebuild,
            next_retry_at: snapshot.retry_wall.filter(|_| backoff),
            providers,
        }
    }
    /// Mode write behind the scan gate; the OS read happens first because it may block briefly.
    pub async fn update_timezone(
        &self,
        request: api::UpdateTimezoneRequest,
    ) -> Result<api::TimezoneStatus, api::ApiError> {
        let (Some(settings), Some(monitor)) = (&self.settings, self.timezone.get()) else {
            return Err(mapping::error(CoreError::UnsupportedFilter));
        };
        let system = match request.mode {
            api::TimezoneMode::FollowSystem => monitor.detect().await.ok(),
            api::TimezoneMode::Fixed => None,
        };
        {
            let active = self.active.lock().await;
            if self.closing.load(Ordering::Acquire) {
                return Err(mapping::error(CoreError::ShuttingDown));
            }
            let (zone, busy) = (system.as_deref(), !active.is_empty());
            if settings.update_timezone(&request, zone, busy).await? {
                monitor.reset_rebuild();
                if let Some(scheduler) = self.scheduler.get() {
                    scheduler.reconfigure(true);
                }
            }
        }
        // Re-detect under the saved mode: Fixed clears a queued change, Follow queues the latest.
        monitor.wake(true);
        self.get_timezone().await
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
        let (result, configs, timezone_changed) = store.update(request).await?;
        *self.configs.write().await = configs;
        if let Some(scheduler) = self.scheduler.get() {
            scheduler.reconfigure(true);
        }
        if let Some(monitor) = self.timezone.get() {
            if timezone_changed {
                monitor.reset_rebuild();
            }
            // Enabled sources may have changed; a legacy zone choice also replaces a queued one.
            monitor.wake(timezone_changed);
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
            // The scheduler now owns any pending rebuild; the monitor's backoff no longer applies.
            let owner_changed = enabled && !previous.enabled;
            if let (true, Some(monitor)) = (owner_changed, self.timezone.get()) {
                monitor.reset_rebuild();
            }
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
        // Rebuild ownership moves between the scheduler and the timezone monitor.
        self.wake_timezone(false);
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
        if let Some(monitor) = self.timezone.get() {
            monitor.stop().await;
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
        if let Some(monitor) = self.timezone.get() {
            monitor.begin_stop();
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
    use crate::profile::DesktopProfile;
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
    type Log = Arc<std::sync::Mutex<Vec<(String, String)>>>;
    /// Synthetic source: logs every read and its directory, waits while `hold` is set and fails
    /// while `fail` is set.
    struct Controlled {
        id: &'static str,
        log: Log,
        roots: Log,
        hold: Arc<AtomicBool>,
        fail: Arc<AtomicBool>,
    }
    #[async_trait::async_trait]
    impl UsageSource for Controlled {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                provider_id: self.id.into(),
                ..Source.descriptor()
            }
        }
        async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError> {
            Source.detect(config).await
        }
        async fn collect(
            &self,
            request: CollectRequest,
            cancellation: CancellationToken,
        ) -> Result<CollectionBatch, CoreError> {
            self.log
                .lock()
                .unwrap()
                .push((self.id.into(), request.timezone.clone()));
            let root = request.config.root_path.clone().unwrap_or_default();
            self.roots.lock().unwrap().push((self.id.into(), root));
            while self.hold.load(Ordering::Acquire) {
                cancellation.check()?;
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            if self.fail.load(Ordering::Acquire) {
                return Err(CoreError::CollectionFailed);
            }
            Ok(CollectionBatch {
                snapshots: vec![snapshot(self.id, &request.timezone)],
            })
        }
    }
    fn snapshot(provider: &str, zone: &str) -> ReportSnapshot {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-04T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        ReportSnapshot {
            key: SnapshotKey {
                product_id: "claude-code".into(),
                source_dataset_id: format!("{provider}-dataset"),
                report_kind: ReportKind::Daily,
                timezone: zone.into(),
                scope: QueryScope::Standard,
            },
            provider_id: provider.into(),
            origin_device_id: "device".into(),
            revision: 0,
            collected_at: at,
            collection_started_at: at,
            collector_version: "test".into(),
            normalization_version: "test".into(),
            coverage: Coverage {
                state: CoverageState::Complete,
                range: None,
                observed_from: None,
                observed_until: None,
            },
            warnings: vec![],
            rows: vec![],
        }
    }
    /// Controllable OS port; tests never read or change the computer's timezone.
    struct Zone {
        current: std::sync::Mutex<Result<String, DetectionError>>,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl crate::timezone::SystemTimezone for Zone {
        fn detect(&self) -> Result<String, DetectionError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.current.lock().unwrap().clone()
        }
    }
    struct Harness {
        runtime: Arc<Runtime>,
        zone: Arc<Zone>,
        log: Log,
        roots: Log,
        hold: Arc<AtomicBool>,
        fail: Arc<AtomicBool>,
        emitted: Arc<std::sync::Mutex<Vec<api::TimezoneStatus>>>,
        directory: tempfile::TempDir,
    }
    impl Harness {
        fn new(directory: tempfile::TempDir, profile: DesktopProfile, enabled: &[&str]) -> Self {
            let configured = ["alpha", "beta", "gamma"].map(|id| {
                let config = SourceConfig {
                    enabled: enabled.contains(&id),
                    root_path: None,
                };
                (id, config)
            });
            Self::with_sources(directory, profile, configured.into())
        }
        /// One source per entry; settings saves need the profile's own provider IDs.
        fn with_sources(
            directory: tempfile::TempDir,
            profile: DesktopProfile,
            configured: Vec<(&'static str, SourceConfig)>,
        ) -> Self {
            let log = Log::default();
            let roots = Log::default();
            let hold = Arc::new(AtomicBool::new(false));
            let fail = Arc::new(AtomicBool::new(false));
            let mut sources: Vec<Arc<dyn UsageSource>> = vec![];
            let mut configs = BTreeMap::new();
            for (id, config) in configured {
                sources.push(Arc::new(Controlled {
                    id,
                    log: log.clone(),
                    roots: roots.clone(),
                    hold: hold.clone(),
                    fail: fail.clone(),
                }));
                configs.insert(id.to_owned(), config);
            }
            let service = Arc::new(UsageService::new(
                Arc::new(MemoryRepository::default()),
                Arc::new(FixedClock(
                    chrono::DateTime::parse_from_rfc3339("2026-10-04T12:00:00Z")
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                )),
                sources,
            ));
            let zone = Arc::new(Zone {
                current: std::sync::Mutex::new(Ok(profile.timezone.clone())),
                calls: Default::default(),
            });
            let store = Arc::new(SettingsStore::new(directory.path(), profile));
            let emit: ScanEmitter = Arc::new(|_| {});
            let runtime = Runtime::new_with_settings(service, configs, emit, Some(store));
            let emitted = Arc::new(std::sync::Mutex::new(vec![]));
            let sink = emitted.clone();
            let monitor = TimezoneMonitor::new(
                zone.clone(),
                Arc::new(move |status| sink.lock().unwrap().push(status)),
            );
            runtime.timezone.set(monitor).ok().unwrap();
            Self {
                runtime,
                zone,
                log,
                roots,
                hold,
                fail,
                emitted,
                directory,
            }
        }
        fn monitor(&self) -> &Arc<TimezoneMonitor> {
            self.runtime.timezone.get().unwrap()
        }
        fn system(&self, zone: Result<&str, DetectionError>) {
            *self.zone.current.lock().unwrap() = zone.map(Into::into);
        }
        fn reads(&self) -> Vec<(String, String)> {
            self.log.lock().unwrap().clone()
        }
        /// The directory each read used; empty for a source's default location.
        fn roots(&self) -> Vec<(String, String)> {
            self.roots.lock().unwrap().clone()
        }
        /// Waits until a spawned job is really inside collect, so holds and cancels are meaningful.
        async fn reached(&self, expected: (String, String)) {
            tokio::time::timeout(Duration::from_secs(3), async {
                while !self.reads().contains(&expected) {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
        }
        async fn scan(&self, zone: &str) -> String {
            self.runtime
                .start_scan(job("alpha", zone))
                .await
                .unwrap()
                .job_id
        }
        async fn status(&self) -> api::TimezoneStatus {
            self.runtime.get_timezone().await.unwrap()
        }
        /// One deterministic monitor step without the background loop.
        async fn tick(&self) -> api::TimezoneStatus {
            let detection = self.monitor().detect().await;
            self.runtime.observe_system_timezone(detection).await;
            self.runtime.drive_timezone().await;
            self.status().await
        }
        async fn settle(&self) -> api::TimezoneStatus {
            self.runtime.wait_for_idle().await;
            self.tick().await
        }
    }
    fn prepared(directory: &tempfile::TempDir, mode: TimezoneMode, zone: &str) -> DesktopProfile {
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone_mode = mode;
        profile.timezone = zone.into();
        profile
    }
    fn harness(mode: TimezoneMode, zone: &str, enabled: &[&str]) -> Harness {
        let directory = tempfile::tempdir().unwrap();
        let profile = prepared(&directory, mode, zone);
        Harness::new(directory, profile, enabled)
    }
    fn job(provider: &str, zone: &str) -> api::StartScanRequest {
        api::StartScanRequest {
            provider_id: provider.into(),
            timezone: zone.into(),
        }
    }
    fn read(provider: &str, zone: &str) -> (String, String) {
        (provider.into(), zone.into())
    }
    fn timezone_request(
        revision: &str,
        mode: api::TimezoneMode,
        zone: Option<&str>,
    ) -> api::UpdateTimezoneRequest {
        api::UpdateTimezoneRequest {
            expected_revision: revision.into(),
            mode,
            timezone: zone.map(Into::into),
        }
    }
    #[tokio::test]
    async fn follow_mode_rebuilds_enabled_sources_serially_without_enabling_auto() {
        let (phoenix, tokyo) = ("America/Phoenix", "Asia/Tokyo");
        let h = harness(TimezoneMode::FollowSystem, phoenix, &["alpha", "beta"]);
        let status = h.tick().await;
        assert_eq!(status.system_timezone.as_deref(), Some(phoenix));
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        h.hold.store(true, Ordering::Release);
        h.system(Ok(tokyo));
        let status = h.tick().await;
        assert_eq!(status.effective_timezone, tokyo);
        assert_eq!(status.revision, "2");
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Rebuilding);
        assert_eq!(status.providers.len(), 2);
        h.reached(read("alpha", tokyo)).await;
        // Serial: the second source waits while the first rebuild is still collecting.
        let status = h.tick().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Rebuilding);
        assert_eq!(h.runtime.active.lock().await.len(), 1);
        h.hold.store(false, Ordering::Release);
        h.settle().await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        assert!(status.providers.is_empty());
        // Serial, enabled-only and never relabelled: each source is re-read in the new zone.
        let expected = [read("alpha", tokyo), read("beta", tokyo)];
        assert_eq!(h.reads(), expected);
        let settings = h.runtime.settings.as_ref().unwrap();
        assert!(!settings.auto_config().await.2.enabled);
        let emitted = h.emitted.lock().unwrap().clone();
        let sequences: Vec<u64> = emitted
            .iter()
            .map(|s| s.sequence.parse().unwrap())
            .collect();
        assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(emitted.iter().any(|s| s.effective_timezone == tokyo));
    }
    #[tokio::test]
    async fn fixed_mode_records_but_ignores_system_changes() {
        let h = harness(TimezoneMode::Fixed, "America/Phoenix", &["alpha"]);
        h.system(Ok("Asia/Tokyo"));
        let status = h.tick().await;
        assert_eq!(status.mode, api::TimezoneMode::Fixed);
        assert_eq!(status.effective_timezone, "America/Phoenix");
        assert_eq!(status.system_timezone.as_deref(), Some("Asia/Tokyo"));
        assert!(status.pending_timezone.is_none());
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        assert!(h.reads().is_empty());
    }
    #[tokio::test]
    async fn failed_or_invalid_detection_keeps_the_last_valid_zone_and_retries() {
        let h = harness(TimezoneMode::FollowSystem, "America/Phoenix", &[]);
        for error in [DetectionError::Unavailable, DetectionError::Invalid] {
            h.system(Err(error));
            let status = h.tick().await;
            assert_eq!(status.effective_timezone, "America/Phoenix");
            assert_eq!(status.revision, "1");
            assert!(status.detection_error.is_some());
        }
        h.system(Ok("UTC"));
        let status = h.tick().await;
        assert_eq!(status.effective_timezone, "UTC");
        assert!(status.detection_error.is_none());
    }
    #[tokio::test]
    async fn changes_during_a_scan_wait_and_rapid_changes_resolve_to_the_latest() {
        let phoenix = "America/Phoenix";
        let h = harness(TimezoneMode::FollowSystem, phoenix, &["alpha"]);
        h.hold.store(true, Ordering::Release);
        let manual = h.scan(phoenix).await;
        for zone in ["Asia/Tokyo", "Europe/Paris", "UTC"] {
            h.system(Ok(zone));
            let status = h.tick().await;
            assert_eq!(status.effective_timezone, phoenix);
            assert_eq!(status.pending_timezone.as_deref(), Some(zone));
        }
        // The running job keeps its captured scope.
        let jobs = h.runtime.active.lock().await.clone();
        assert_eq!(jobs["alpha"].timezone, phoenix);
        h.hold.store(false, Ordering::Release);
        h.runtime.wait_for_idle().await;
        let finished = h.runtime.get_scan(&manual).await.unwrap();
        assert_eq!(finished.state, ScanState::Succeeded);
        let status = h.tick().await;
        assert_eq!(status.effective_timezone, "UTC");
        assert_eq!(status.revision, "2");
        assert!(status.pending_timezone.is_none());
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        let expected = [read("alpha", phoenix), read("alpha", "UTC")];
        assert_eq!(h.reads(), expected);
    }
    #[tokio::test]
    async fn cancelled_or_failed_rebuilds_keep_history_back_off_and_need_the_current_zone() {
        let (phoenix, tokyo) = ("America/Phoenix", "Asia/Tokyo");
        let h = harness(TimezoneMode::FollowSystem, phoenix, &["alpha"]);
        h.scan(phoenix).await;
        h.runtime.wait_for_idle().await;
        h.hold.store(true, Ordering::Release);
        h.system(Ok(tokyo));
        let status = h.tick().await;
        let rebuild = status.providers[0].job_id.clone().unwrap();
        h.reached(read("alpha", tokyo)).await;
        h.runtime.cancel_scan(&rebuild).await.unwrap();
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Backoff);
        assert!(status.next_retry_at.is_some());
        let step = status.providers[0].state;
        assert_eq!(step, api::TimezoneProviderState::Failed);
        // No immediate cancellation/retry loop.
        assert!(h.runtime.active.lock().await.is_empty());
        // A success in the previous zone never acknowledges the new target.
        h.hold.store(false, Ordering::Release);
        h.scan(phoenix).await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Backoff);
        h.fail.store(true, Ordering::Release);
        h.monitor().advance(300);
        h.settle().await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Backoff);
        assert!(h.runtime.active.lock().await.is_empty());
        let repository = &h.runtime.service.repository;
        let filter = SnapshotFilter::default();
        let stored = repository.load_snapshots(filter).await.unwrap();
        let zones: Vec<_> = stored.into_iter().map(|s| s.key.timezone).collect();
        assert_eq!(zones, [phoenix]);
        h.fail.store(false, Ordering::Release);
        h.monitor().advance(120);
        h.settle().await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        let reads = h.reads();
        assert_eq!(reads.iter().filter(|read| read.1 == tokyo).count(), 3);
    }
    #[tokio::test]
    async fn a_source_moved_to_a_new_directory_is_rebuilt_from_it_before_completion() {
        use api::TimezoneProviderState as Step;
        let (phoenix, tokyo) = ("America/Phoenix", "Asia/Tokyo");
        let [claude, codex, _] = crate::profile::PROVIDERS;
        let directory = tempfile::tempdir().unwrap();
        let mut profile = prepared(&directory, TimezoneMode::FollowSystem, phoenix);
        for id in [claude, codex] {
            let mut provider = profile.provider(id).unwrap();
            provider.enabled = true;
            profile.set_provider(id, provider).unwrap();
        }
        let dataset = profile.claude_dataset_id.clone();
        let configs = profile.configs();
        let configured = crate::profile::PROVIDERS.map(|id| (id, configs[id].clone()));
        let h = Harness::with_sources(directory, profile, configured.into());
        let steps = |status: api::TimezoneStatus| -> Vec<_> {
            status.providers.into_iter().map(|p| p.state).collect()
        };
        // Automatic collection stays off: A is rebuilt in the new zone, then B fails and backs off.
        h.hold.store(true, Ordering::Release);
        h.system(Ok(tokyo));
        h.tick().await;
        h.reached(read(claude, tokyo)).await;
        h.hold.store(false, Ordering::Release);
        h.runtime.wait_for_idle().await;
        h.fail.store(true, Ordering::Release);
        h.tick().await;
        h.runtime.wait_for_idle().await;
        let status = h.status().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Backoff);
        assert_eq!(steps(status), [Step::Succeeded, Step::Failed]);
        // With no scan running, saving A unchanged keeps its progress; a new directory withdraws it.
        let save = |revision: &str, directory_ref: Option<String>| api::UpdateSettingsRequest {
            expected_revision: revision.into(),
            timezone: tokyo.into(),
            providers: vec![api::ProviderSettingsUpdate {
                provider_id: claude.into(),
                enabled: true,
                directory_ref,
            }],
        };
        h.runtime.update_settings(save("2", None)).await.unwrap();
        assert_eq!(steps(h.status().await), [Step::Succeeded, Step::Failed]);
        let logs = h.directory.path().join("moved").join("projects");
        std::fs::create_dir_all(&logs).unwrap();
        let moved = h.runtime.remember_directory(claude, logs).await.unwrap();
        h.runtime
            .update_settings(save("3", Some(moved.directory_ref)))
            .await
            .unwrap();
        assert_eq!(steps(h.status().await), [Step::Pending, Step::Failed]);
        // B succeeding is not enough while A has not been read from its new directory.
        h.fail.store(false, Ordering::Release);
        h.runtime.start_scan(job(codex, tokyo)).await.unwrap();
        h.runtime.wait_for_idle().await;
        let status = h.status().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Backoff);
        assert_eq!(steps(status), [Step::Pending, Step::Succeeded]);
        let stored = DesktopProfile::load_or_create(h.directory.path()).unwrap();
        assert!(stored.timezone_needs_rescan);
        // After the backoff, the serial rebuild reads A from the new directory and completes.
        h.monitor().advance(60);
        h.settle().await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        let stored = DesktopProfile::load_or_create(h.directory.path()).unwrap();
        assert!(!stored.timezone_needs_rescan);
        assert_eq!(stored.claude_dataset_id, dataset);
        let reads = [claude, codex, codex, claude].map(|id| read(id, tokyo));
        assert_eq!(h.reads(), reads);
        // Only the last read used a custom directory: A's new one.
        let roots = h.roots();
        assert!(roots[..3].iter().all(|(_, root)| root.is_empty()));
        assert_eq!(roots[3], read(claude, &stored.claude_root_path.unwrap()));
    }
    #[tokio::test]
    async fn automatic_collection_owns_the_rebuild_when_enabled() {
        let (id, tokyo, phoenix) = ("alpha", "Asia/Tokyo", "America/Phoenix");
        let directory = tempfile::tempdir().unwrap();
        let mut profile = prepared(&directory, TimezoneMode::FollowSystem, phoenix);
        profile.auto_collection.enabled = true;
        let h = Harness::new(directory, profile, &["alpha"]);
        h.system(Ok(tokyo));
        let status = h.tick().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Pending);
        assert!(h.runtime.active.lock().await.is_empty());
        let configs = h.runtime.configs.read().await.clone();
        let (runtime, config) = (&h.runtime, &configs[id]);
        let started = runtime.start_automatic_scan(id, tokyo, config).await;
        assert!(started.unwrap().is_some());
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        assert_eq!(h.reads(), [read("alpha", tokyo)]);
    }
    #[tokio::test]
    async fn no_enabled_sources_switch_the_zone_without_collection() {
        let h = harness(TimezoneMode::FollowSystem, "America/Phoenix", &[]);
        h.system(Ok("Asia/Tokyo"));
        let status = h.tick().await;
        assert_eq!(status.effective_timezone, "Asia/Tokyo");
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        assert!(h.reads().is_empty());
    }
    #[tokio::test]
    async fn a_pending_rebuild_resumes_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = prepared(&directory, TimezoneMode::Fixed, "Asia/Tokyo");
        profile.timezone_needs_rescan = true;
        profile.persist(directory.path()).unwrap();
        let reopened = DesktopProfile::load_or_create(directory.path()).unwrap();
        let h = Harness::new(directory, reopened, &["alpha"]);
        h.tick().await;
        let status = h.settle().await;
        assert_eq!(status.rebuild, api::TimezoneRebuildState::Idle);
        assert_eq!(h.reads(), [read("alpha", "Asia/Tokyo")]);
        let stored = DesktopProfile::load_or_create(h.directory.path()).unwrap();
        assert!(!stored.timezone_needs_rescan);
    }
    #[tokio::test]
    async fn switching_to_follow_during_a_scan_waits_for_the_safe_boundary() {
        let (phoenix, tokyo) = ("America/Phoenix", "Asia/Tokyo");
        let h = harness(TimezoneMode::Fixed, phoenix, &["alpha"]);
        h.system(Ok(tokyo));
        h.hold.store(true, Ordering::Release);
        h.scan(phoenix).await;
        let fixed = timezone_request("1", api::TimezoneMode::Fixed, Some("UTC"));
        let busy = h.runtime.update_timezone(fixed).await.unwrap_err();
        assert_eq!(busy.code, api::ErrorCode::ScanBusy);
        let follow = timezone_request("1", api::TimezoneMode::FollowSystem, None);
        let saved = h.runtime.update_timezone(follow).await.unwrap();
        assert_eq!(saved.mode, api::TimezoneMode::FollowSystem);
        assert_eq!(saved.effective_timezone, phoenix);
        let status = h.tick().await;
        assert_eq!(status.pending_timezone.as_deref(), Some(tokyo));
        h.hold.store(false, Ordering::Release);
        let status = h.settle().await;
        assert_eq!(status.effective_timezone, tokyo);
        h.runtime.shutdown().await;
    }
    #[tokio::test]
    async fn the_native_loop_checks_at_start_and_stops_before_runtime_shutdown() {
        let h = harness(TimezoneMode::FollowSystem, "America/Phoenix", &[]);
        h.system(Ok("Asia/Tokyo"));
        h.monitor().start(&h.runtime).await;
        tokio::time::timeout(Duration::from_secs(3), async {
            while h.status().await.effective_timezone != "Asia/Tokyo" {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        h.runtime.shutdown().await;
        assert!(h.runtime.is_exit_ready());
        let calls = h.zone.calls.load(Ordering::Acquire);
        h.system(Ok("Europe/Paris"));
        h.runtime.wake_timezone(true);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(h.zone.calls.load(Ordering::Acquire), calls);
        assert_eq!(h.status().await.effective_timezone, "Asia/Tokyo");
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
