//! Single native driver, bounded per-provider pending state and serial automatic full scans.
mod policy;
use crate::{mapping, runtime::Runtime};
use policy::Policy;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
    time::Instant,
};
use usage_contracts as api;
use usage_core::{CancellationToken, CoreError, ScanState, SourceConfig, SourceWatch};

type Emitter = Arc<dyn Fn(api::AutoCollectionStatus) + Send + Sync>;
struct Entry {
    epoch: u64,
    scope: String,
    config: SourceConfig,
    policy: Policy,
    watch: Option<Box<dyn SourceWatch>>,
    observation_scope: Option<String>,
    hints: Arc<AtomicBool>,
    status: api::AutoProviderStatus,
}
struct Job {
    provider: String,
    id: String,
    before: String,
    generation: u64,
    epoch: u64,
}
pub struct Scheduler {
    stopping: AtomicBool,
    wake: Notify,
    task: Mutex<Option<JoinHandle<()>>>,
    pulse: Mutex<Option<JoinHandle<()>>>,
    resumed: AtomicBool,
    status: Mutex<Option<api::AutoCollectionStatus>>,
    emit: Emitter,
    reset: AtomicBool,
    checks: std::sync::Mutex<CancellationToken>,
}
impl Scheduler {
    pub fn new(emit: Emitter) -> Arc<Self> {
        Arc::new(Self {
            stopping: AtomicBool::new(false),
            wake: Notify::new(),
            task: Mutex::new(None),
            pulse: Mutex::new(None),
            resumed: AtomicBool::new(false),
            status: Mutex::new(None),
            emit,
            reset: AtomicBool::new(false),
            checks: std::sync::Mutex::new(CancellationToken::default()),
        })
    }
    pub async fn start(self: &Arc<Self>, runtime: &Arc<Runtime>) {
        let pulse = self.clone();
        *self.pulse.lock().await = Some(tokio::spawn(async move {
            pulse.watch_resumes().await;
        }));
        let scheduler = self.clone();
        let runtime = Arc::downgrade(runtime);
        *self.task.lock().await = Some(tokio::spawn(async move {
            scheduler.run(runtime).await;
        }));
    }
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    pub fn reconfigure(&self, reset: bool) {
        if reset {
            self.reset.store(true, Ordering::Release);
        }
        if let Ok(mut checks) = self.checks.lock() {
            checks.cancel();
            *checks = CancellationToken::default();
        }
        self.wake();
    }
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(checks) = self.checks.lock() {
            checks.cancel();
        }
        self.wake();
    }
    pub async fn stop(&self) {
        self.begin_stop();
        if let Some(pulse) = self.pulse.lock().await.take() {
            pulse.abort();
            let _ = pulse.await;
        }
        if let Some(mut task) = self.task.lock().await.take() {
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
    pub async fn status(&self) -> Option<api::AutoCollectionStatus> {
        self.status.lock().await.clone()
    }
    /// Independent pulse: source inspection can take seconds without impersonating sleep.
    async fn watch_resumes(&self) {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut previous_mono = Instant::now();
        let mut previous_wall = chrono::Utc::now();
        loop {
            ticker.tick().await;
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            let mono = Instant::now();
            let wall = chrono::Utc::now();
            if policy::resumed(
                mono.duration_since(previous_mono).as_secs(),
                (wall - previous_wall).num_seconds(),
            ) {
                self.resumed.store(true, Ordering::Release);
                self.wake();
            }
            previous_mono = mono;
            previous_wall = wall;
        }
    }
    async fn publish(&self, status: api::AutoCollectionStatus) {
        let mut current = self.status.lock().await;
        if current
            .as_ref()
            .is_none_or(|old| serde_json::to_value(old).ok() != serde_json::to_value(&status).ok())
        {
            *current = Some(status.clone());
            (self.emit)(status);
        }
    }
    async fn run(&self, runtime: Weak<Runtime>) {
        let origin = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
        let mut job: Option<Job> = None;
        let mut previous_interval = 0;
        let mut epoch = 0;
        loop {
            tokio::select! { _ = ticker.tick() => {}, _ = self.wake.notified() => {} }
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            let Some(runtime) = runtime.upgrade() else {
                break;
            };
            let now = origin.elapsed().as_secs();
            let resumed = self.resumed.swap(false, Ordering::AcqRel);
            let Some((revision, timezone, config, configs)) = runtime.auto_context().await else {
                continue;
            };
            let interval = u64::from(config.interval_minutes) * 60;
            if !config.enabled || self.reset.swap(false, Ordering::AcqRel) {
                entries.clear();
            }
            entries.retain(|id, _| config.enabled && configs.get(id).is_some_and(|c| c.enabled));
            if config.enabled {
                for (provider, source_config) in configs.iter().filter(|(_, c)| c.enabled) {
                    let scope = format!("{timezone}|{:?}", source_config.root_path);
                    if entries
                        .get(provider)
                        .is_none_or(|entry| entry.scope != scope)
                    {
                        epoch += 1;
                        let success = runtime
                            .service
                            .repository
                            .list_scans(provider)
                            .await
                            .ok()
                            .and_then(|scans| {
                                scans
                                    .into_iter()
                                    .filter(|scan| scan.state == ScanState::Succeeded)
                                    .filter_map(|scan| scan.finished_at)
                                    .max()
                            })
                            .map(|at| at.to_rfc3339());
                        entries.insert(
                            provider.clone(),
                            Entry {
                                epoch,
                                scope,
                                config: source_config.clone(),
                                policy: Policy::new(now),
                                watch: None,
                                observation_scope: None,
                                hints: Arc::new(AtomicBool::new(false)),
                                status: api::AutoProviderStatus {
                                    provider_id: provider.clone(),
                                    state: api::AutoCollectionState::Waiting,
                                    job_id: None,
                                    last_success_at: success,
                                    next_check_at: None,
                                    watching: false,
                                    error: None,
                                },
                            },
                        );
                    }
                }
            }
            for entry in entries.values_mut() {
                if resumed {
                    // Native buffers may be lost across suspension, so conservatively recheck content.
                    entry.policy.request(now, true);
                }
                if previous_interval != 0 && previous_interval != interval {
                    entry.policy.next_check = now + interval;
                }
                if entry.hints.swap(false, Ordering::AcqRel) {
                    entry.policy.request(now, true);
                }
            }
            previous_interval = interval;
            if let Some(active) = &job {
                match runtime.get_scan(&active.id).await {
                    Ok(scan) if scan.state.is_terminal() => {
                        if let Some(entry) = entries
                            .get_mut(&active.provider)
                            .filter(|e| e.epoch == active.epoch)
                        {
                            match scan.state {
                                ScanState::Succeeded => {
                                    entry
                                        .policy
                                        .succeeded(active.before.clone(), active.generation);
                                    entry.status.last_success_at =
                                        scan.finished_at.map(|at| at.to_rfc3339());
                                    entry.status.error = None;
                                }
                                ScanState::Cancelled => entry.policy.cancelled(now, interval),
                                _ => entry.policy.failed(now, interval),
                            }
                            entry.status.error = scan.error.map(mapping::error);
                            entry.status.job_id = None;
                        }
                        job = None;
                    }
                    Err(error) => {
                        if let Some(entry) = entries.get_mut(&active.provider) {
                            entry.policy.failed(now, interval);
                            entry.status.error = Some(mapping::error(error));
                        }
                        // Retain ownership until a terminal record is observable; never overlap it.
                    }
                    _ => {}
                }
            }
            // Give never-scanned/least-recently-started sources priority, even if one input
            // keeps changing throughout a scan longer than its configured interval.
            let mut candidates: Vec<_> = entries.keys().cloned().collect();
            candidates.sort_by_key(|id| entries[id].policy.last_started);
            for provider in &candidates {
                let entry = entries
                    .get_mut(provider)
                    .expect("candidate belongs to this tick");
                if self.stopping.load(Ordering::Acquire) || self.reset.load(Ordering::Acquire) {
                    break;
                }
                if job.is_some() || runtime.has_active_jobs().await {
                    continue;
                }
                if !entry.policy.due(now) {
                    continue;
                }
                if !entry.policy.scan_allowed(now, interval) {
                    let eligible = entry
                        .policy
                        .last_started
                        .map_or(now, |at| at + interval)
                        .max(entry.policy.retry_at);
                    entry.policy.pending = Some(eligible);
                    entry.policy.next_check = entry.policy.next_check.max(eligible);
                    continue;
                }
                if entry.watch.is_none() {
                    if let Ok(source) = runtime.service.source(provider) {
                        let hints = entry.hints.clone();
                        let source = source.clone();
                        let cfg = entry.config.clone();
                        entry.watch = tokio::task::spawn_blocking(move || {
                            source.watch(
                                &cfg,
                                Arc::new(move || {
                                    hints.store(true, Ordering::Release);
                                }),
                            )
                        })
                        .await
                        .ok()
                        .and_then(Result::ok);
                        entry.status.watching = entry.watch.is_some();
                    }
                }
                entry.status.state = api::AutoCollectionState::Checking;
                let mut status = self
                    .status()
                    .await
                    .unwrap_or_else(|| self.make_status(&revision, &timezone, &config, &[]));
                status.providers.retain(|p| p.provider_id != *provider);
                status.revision = revision.clone();
                status.config = config.clone();
                status.timezone = timezone.clone();
                status.providers.push(entry.status.clone());
                self.publish(status).await;
                let checks = self
                    .checks
                    .lock()
                    .map(|checks| checks.clone())
                    .unwrap_or_default();
                let observation = match runtime.service.source(provider) {
                    Ok(source) => {
                        match tokio::time::timeout(
                            Duration::from_secs(30),
                            source.inspect(
                                usage_core::CollectRequest {
                                    timezone: timezone.clone(),
                                    config: entry.config.clone(),
                                },
                                checks.clone(),
                            ),
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(_) => {
                                checks.cancel();
                                if let Ok(mut stored) = self.checks.lock() {
                                    if stored.is_cancelled()
                                        && !self.stopping.load(Ordering::Acquire)
                                    {
                                        *stored = CancellationToken::default();
                                    }
                                }
                                Err(CoreError::Timeout)
                            }
                        }
                    }
                    Err(error) => Err(error),
                };
                let now = origin.elapsed().as_secs();
                entry.policy.checked(now, interval);
                match observation {
                    Err(error) => {
                        if error == CoreError::Cancelled {
                            continue;
                        }
                        entry.policy.failed(now, interval);
                        entry.watch = None;
                        entry.status.watching = false;
                        entry.status.error = Some(mapping::error(error));
                    }
                    Ok(observation) => {
                        if entry
                            .observation_scope
                            .as_ref()
                            .is_some_and(|scope| *scope != observation.scope)
                        {
                            entry.watch = None;
                            entry.status.watching = false;
                        }
                        entry.observation_scope = Some(observation.scope.clone());
                        let before = format!("{}|{}", observation.scope, observation.fingerprint);
                        // With no functioning watcher, same-metadata content edits are uncertain.
                        if entry.watch.is_none() {
                            entry.policy.request(now, true);
                        }
                        if entry.policy.changed(&before) {
                            if job.is_none()
                                && entry.policy.scan_allowed(now, interval)
                                && !self.stopping.load(Ordering::Acquire)
                            {
                                // Atomically capture/validate configuration and reserve Runtime's slot.
                                match runtime
                                    .start_automatic_scan(provider, &timezone, &entry.config)
                                    .await
                                {
                                    Ok(Some(result)) => {
                                        // Round up the actual start acknowledgement, so even a slow
                                        // metadata check/storage write cannot shorten the minimum gap.
                                        let generation =
                                            entry.policy.started(origin.elapsed().as_secs() + 1);
                                        entry.status.job_id = Some(result.job_id.clone());
                                        job = Some(Job {
                                            provider: provider.clone(),
                                            id: result.job_id,
                                            before,
                                            generation,
                                            epoch: entry.epoch,
                                        });
                                    }
                                    Ok(None) | Err(CoreError::ScanBusy) => {
                                        entry.policy.request(now, false)
                                    }
                                    Err(error) => {
                                        entry.policy.failed(now, interval);
                                        entry.status.error = Some(mapping::error(error));
                                    }
                                }
                            } else {
                                entry.policy.request(now, false);
                                entry.policy.pending = Some(
                                    entry
                                        .policy
                                        .last_started
                                        .map_or(now + 2, |at| (at + interval).max(now + 2)),
                                );
                            }
                        } else {
                            entry.status.error = None;
                        }
                    }
                }
            }
            let now = origin.elapsed().as_secs();
            let wall = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0).unwrap();
            let statuses: Vec<_> = entries
                .values_mut()
                .map(|entry| {
                    entry.status.state = if entry.status.job_id.is_some() {
                        api::AutoCollectionState::Scanning
                    } else if now < entry.policy.retry_at {
                        api::AutoCollectionState::Backoff
                    } else if entry.policy.pending.is_some() {
                        api::AutoCollectionState::Waiting
                    } else {
                        api::AutoCollectionState::Idle
                    };
                    let deadline = entry.policy.deadline();
                    entry.status.next_check_at = Some(
                        (wall + chrono::Duration::seconds(deadline.saturating_sub(now) as i64))
                            .to_rfc3339(),
                    );
                    entry.status.clone()
                })
                .collect();
            self.publish(self.make_status(&revision, &timezone, &config, &statuses))
                .await;
        }
        // Drop watchers before Runtime cancels/reaps collection children.
        entries.clear();
        self.stopping.store(true, Ordering::Release);
    }
    fn make_status(
        &self,
        revision: &str,
        timezone: &str,
        config: &api::AutoCollectionConfig,
        providers: &[api::AutoProviderStatus],
    ) -> api::AutoCollectionStatus {
        api::AutoCollectionStatus {
            api_version: api::API_VERSION.into(),
            revision: revision.into(),
            timezone: timezone.into(),
            config: config.clone(),
            providers: providers.to_vec(),
        }
    }
}
