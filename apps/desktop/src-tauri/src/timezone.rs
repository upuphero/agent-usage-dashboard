//! Native statistical-timezone policy: IANA identity, OS detection and one background monitor.
//! Identity is the canonical zone name, never the current UTC offset, so DST is not a change.
use crate::{mapping, runtime::Runtime};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, MutexGuard, Notify},
    task::JoinHandle,
    time::Instant,
};
use usage_contracts as api;
use usage_core::CoreError;

/// Lightweight polling fallback; window focus and resume trigger an earlier check.
pub const POLL_SECONDS: u64 = 60;
const TICK_SECONDS: u64 = 5;
const DETECT_TIMEOUT: Duration = Duration::from_secs(5);
// Legacy spellings an OS may report for the same setting. Only renamed/backward links are
// listed; distinct cities that tzdata merged as links (for example Europe/Oslo) keep their name.
// The GMT forms join UTC: both are permanently offset zero without DST.
const ALIASES: &[(&str, &str)] = &[
    ("Etc/UTC", "UTC"),
    ("Etc/UCT", "UTC"),
    ("UCT", "UTC"),
    ("Etc/Universal", "UTC"),
    ("Universal", "UTC"),
    ("Etc/Zulu", "UTC"),
    ("Zulu", "UTC"),
    ("Etc/GMT", "UTC"),
    ("GMT", "UTC"),
    ("Etc/GMT0", "UTC"),
    ("Etc/GMT+0", "UTC"),
    ("Etc/GMT-0", "UTC"),
    ("GMT0", "UTC"),
    ("GMT+0", "UTC"),
    ("GMT-0", "UTC"),
    ("Etc/Greenwich", "UTC"),
    ("Greenwich", "UTC"),
    ("Africa/Asmera", "Africa/Asmara"),
    ("America/Atka", "America/Adak"),
    ("America/Buenos_Aires", "America/Argentina/Buenos_Aires"),
    ("America/Catamarca", "America/Argentina/Catamarca"),
    ("America/Cordoba", "America/Argentina/Cordoba"),
    ("America/Ensenada", "America/Tijuana"),
    ("America/Fort_Wayne", "America/Indiana/Indianapolis"),
    ("America/Godthab", "America/Nuuk"),
    ("America/Indianapolis", "America/Indiana/Indianapolis"),
    ("America/Jujuy", "America/Argentina/Jujuy"),
    ("America/Knox_IN", "America/Indiana/Knox"),
    ("America/Louisville", "America/Kentucky/Louisville"),
    ("America/Mendoza", "America/Argentina/Mendoza"),
    ("America/Montreal", "America/Toronto"),
    ("America/Porto_Acre", "America/Rio_Branco"),
    ("America/Santa_Isabel", "America/Tijuana"),
    ("America/Shiprock", "America/Denver"),
    ("Asia/Ashkhabad", "Asia/Ashgabat"),
    ("Asia/Calcutta", "Asia/Kolkata"),
    ("Asia/Chongqing", "Asia/Shanghai"),
    ("Asia/Chungking", "Asia/Shanghai"),
    ("Asia/Dacca", "Asia/Dhaka"),
    ("Asia/Harbin", "Asia/Shanghai"),
    ("Asia/Istanbul", "Europe/Istanbul"),
    ("Asia/Katmandu", "Asia/Kathmandu"),
    ("Asia/Macao", "Asia/Macau"),
    ("Asia/Rangoon", "Asia/Yangon"),
    ("Asia/Saigon", "Asia/Ho_Chi_Minh"),
    ("Asia/Tel_Aviv", "Asia/Jerusalem"),
    ("Asia/Thimbu", "Asia/Thimphu"),
    ("Asia/Ujung_Pandang", "Asia/Makassar"),
    ("Asia/Ulan_Bator", "Asia/Ulaanbaatar"),
    ("Atlantic/Faeroe", "Atlantic/Faroe"),
    ("Australia/ACT", "Australia/Sydney"),
    ("Australia/Canberra", "Australia/Sydney"),
    ("Australia/LHI", "Australia/Lord_Howe"),
    ("Australia/NSW", "Australia/Sydney"),
    ("Australia/North", "Australia/Darwin"),
    ("Australia/Queensland", "Australia/Brisbane"),
    ("Australia/South", "Australia/Adelaide"),
    ("Australia/Tasmania", "Australia/Hobart"),
    ("Australia/Victoria", "Australia/Melbourne"),
    ("Australia/West", "Australia/Perth"),
    ("Australia/Yancowinna", "Australia/Broken_Hill"),
    ("Brazil/Acre", "America/Rio_Branco"),
    ("Brazil/DeNoronha", "America/Noronha"),
    ("Brazil/East", "America/Sao_Paulo"),
    ("Brazil/West", "America/Manaus"),
    ("Canada/Atlantic", "America/Halifax"),
    ("Canada/Central", "America/Winnipeg"),
    ("Canada/Eastern", "America/Toronto"),
    ("Canada/Mountain", "America/Edmonton"),
    ("Canada/Newfoundland", "America/St_Johns"),
    ("Canada/Pacific", "America/Vancouver"),
    ("Canada/Saskatchewan", "America/Regina"),
    ("Canada/Yukon", "America/Whitehorse"),
    ("Chile/Continental", "America/Santiago"),
    ("Chile/EasterIsland", "Pacific/Easter"),
    ("Cuba", "America/Havana"),
    ("Egypt", "Africa/Cairo"),
    ("Eire", "Europe/Dublin"),
    ("Europe/Belfast", "Europe/London"),
    ("Europe/Kiev", "Europe/Kyiv"),
    ("Europe/Nicosia", "Asia/Nicosia"),
    ("Europe/Tiraspol", "Europe/Chisinau"),
    ("GB", "Europe/London"),
    ("GB-Eire", "Europe/London"),
    ("Hongkong", "Asia/Hong_Kong"),
    ("Iceland", "Atlantic/Reykjavik"),
    ("Iran", "Asia/Tehran"),
    ("Israel", "Asia/Jerusalem"),
    ("Jamaica", "America/Jamaica"),
    ("Japan", "Asia/Tokyo"),
    ("Kwajalein", "Pacific/Kwajalein"),
    ("Libya", "Africa/Tripoli"),
    ("Mexico/BajaNorte", "America/Tijuana"),
    ("Mexico/BajaSur", "America/Mazatlan"),
    ("Mexico/General", "America/Mexico_City"),
    ("NZ", "Pacific/Auckland"),
    ("NZ-CHAT", "Pacific/Chatham"),
    ("Navajo", "America/Denver"),
    ("PRC", "Asia/Shanghai"),
    ("Pacific/Enderbury", "Pacific/Kanton"),
    ("Pacific/Johnston", "Pacific/Honolulu"),
    ("Pacific/Ponape", "Pacific/Pohnpei"),
    ("Pacific/Samoa", "Pacific/Pago_Pago"),
    ("Pacific/Truk", "Pacific/Chuuk"),
    ("Pacific/Yap", "Pacific/Chuuk"),
    ("Poland", "Europe/Warsaw"),
    ("Portugal", "Europe/Lisbon"),
    ("ROC", "Asia/Taipei"),
    ("ROK", "Asia/Seoul"),
    ("Singapore", "Asia/Singapore"),
    ("Turkey", "Europe/Istanbul"),
    ("US/Alaska", "America/Anchorage"),
    ("US/Aleutian", "America/Adak"),
    ("US/Arizona", "America/Phoenix"),
    ("US/Central", "America/Chicago"),
    ("US/East-Indiana", "America/Indiana/Indianapolis"),
    ("US/Eastern", "America/New_York"),
    ("US/Hawaii", "Pacific/Honolulu"),
    ("US/Indiana-Starke", "America/Indiana/Knox"),
    ("US/Michigan", "America/Detroit"),
    ("US/Mountain", "America/Denver"),
    ("US/Pacific", "America/Los_Angeles"),
    ("US/Samoa", "Pacific/Pago_Pago"),
    ("W-SU", "Europe/Moscow"),
];

/// Validated canonical IANA identity, or None for anything chrono-tz cannot resolve.
pub fn canonical(zone: &str) -> Option<String> {
    if zone.is_empty() || zone.len() > 64 {
        return None;
    }
    usage_core::application::validate_timezone(zone).ok()?;
    let canonical = ALIASES
        .iter()
        .find(|(alias, _)| *alias == zone)
        .map_or(zone, |(_, target)| *target);
    Some(canonical.into())
}
/// Alias-insensitive identity comparison; a stored legacy spelling is not a new selection.
pub fn same_zone(left: &str, right: &str) -> bool {
    left == right || canonical(left).is_some_and(|left| canonical(right) == Some(left))
}
/// Failed or cancelled rebuilds wait before retrying; cancellation waits at least five minutes.
pub fn retry_delay(failures: u32, cancelled: bool) -> u64 {
    let backoff = (30u64 << failures.clamp(1, 5)).min(900);
    if cancelled {
        backoff.max(300)
    } else {
        backoff
    }
}
/// Wall-clock deadline shown to clients; the backoff itself uses the monotonic clock.
fn wall_after(seconds: u64) -> Option<String> {
    let now = chrono::Utc::now().timestamp();
    let at = chrono::DateTime::from_timestamp(now.saturating_add(seconds as i64), 0)?;
    Some(at.to_rfc3339())
}
/// Monotonic clocks may exclude sleep, so a large wall-clock gap is also a resume hint.
fn resumed(monotonic_gap: u64, wall_gap: i64) -> bool {
    monotonic_gap > 3 * TICK_SECONDS || wall_gap > 3 * TICK_SECONDS as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionError {
    Unavailable,
    Invalid,
    Timeout,
}
impl DetectionError {
    pub fn api(self) -> api::ApiError {
        let (code, message) = match self {
            Self::Unavailable => (
                api::ErrorCode::Internal,
                "无法读取系统时区，继续使用当前统计时区并稍后重试。",
            ),
            Self::Invalid => (
                api::ErrorCode::InvalidData,
                "系统返回的时区无效，继续使用当前统计时区并稍后重试。",
            ),
            Self::Timeout => (
                api::ErrorCode::Timeout,
                "读取系统时区超时，继续使用当前统计时区并稍后重试。",
            ),
        };
        api::ApiError {
            api_version: api::API_VERSION.into(),
            code,
            message: message.into(),
            retryable: true,
        }
    }
}
/// Injected port so tests never read or change the computer's timezone.
pub trait SystemTimezone: Send + Sync {
    fn detect(&self) -> Result<String, DetectionError>;
}
pub struct OsTimezone;
impl SystemTimezone for OsTimezone {
    fn detect(&self) -> Result<String, DetectionError> {
        // Windows reads a fresh Calendar; macOS resets CoreFoundation's cache on every call.
        let zone = iana_time_zone::get_timezone().map_err(|_| DetectionError::Unavailable)?;
        canonical(&zone).ok_or(DetectionError::Invalid)
    }
}

/// Clears the in-flight flag even if the OS read panics or outlives its timeout.
struct InFlight(Arc<AtomicBool>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Default)]
struct MonitorState {
    system: Option<String>,
    detection_error: Option<DetectionError>,
    pending: Option<String>,
    failures: u32,
    retry_at: u64,
    retry_wall: Option<String>,
    errors: BTreeMap<String, api::ApiError>,
}
pub struct MonitorSnapshot {
    pub system: Option<String>,
    pub detection_error: Option<api::ApiError>,
    pub pending: Option<String>,
    pub retry_at: u64,
    pub retry_wall: Option<String>,
    pub errors: BTreeMap<String, api::ApiError>,
}
/// Serializes status computation with its sequence, so an older snapshot never gets a newer number.
#[derive(Default)]
pub struct Published {
    sequence: u64,
    last: Option<serde_json::Value>,
}
impl Published {
    pub fn next(&mut self, mut status: api::TimezoneStatus) -> api::TimezoneStatus {
        self.sequence = self.sequence.saturating_add(1);
        status.sequence = self.sequence.to_string();
        status
    }
    /// Unstamped content comparison; repeated identical polls do not emit events.
    pub fn changed(&mut self, status: &api::TimezoneStatus) -> bool {
        let content = serde_json::to_value(status).ok();
        if content == self.last {
            return false;
        }
        self.last = content;
        true
    }
}
type Emitter = Arc<dyn Fn(api::TimezoneStatus) + Send + Sync>;
pub struct TimezoneMonitor {
    detector: Arc<dyn SystemTimezone>,
    emit: Emitter,
    origin: Instant,
    /// Always zero in the app; tests advance it instead of sleeping through backoff.
    skew: AtomicU64,
    notify: Notify,
    detect_requested: AtomicBool,
    /// Serializes callers; the flag below only marks a read that outlived its timeout.
    serial: Mutex<()>,
    detecting: Arc<AtomicBool>,
    stopping: AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
    state: std::sync::Mutex<MonitorState>,
    published: Mutex<Published>,
}
impl TimezoneMonitor {
    pub fn new(detector: Arc<dyn SystemTimezone>, emit: Emitter) -> Arc<Self> {
        Arc::new(Self {
            detector,
            emit,
            origin: Instant::now(),
            skew: AtomicU64::new(0),
            notify: Notify::new(),
            detect_requested: AtomicBool::new(false),
            serial: Mutex::new(()),
            detecting: Arc::new(AtomicBool::new(false)),
            stopping: AtomicBool::new(false),
            task: Mutex::new(None),
            state: std::sync::Mutex::new(MonitorState::default()),
            published: Mutex::new(Published::default()),
        })
    }
    /// Monotonic seconds since the monitor was created; used for rebuild backoff only.
    pub fn now(&self) -> u64 {
        self.origin.elapsed().as_secs() + self.skew.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub fn advance(&self, seconds: u64) {
        self.skew.fetch_add(seconds, Ordering::AcqRel);
    }
    /// One bounded OS read. Concurrent callers wait their turn; a stuck read is never duplicated.
    pub async fn detect(&self) -> Result<String, DetectionError> {
        let _turn = self.serial.lock().await;
        if self.detecting.swap(true, Ordering::AcqRel) {
            return Err(DetectionError::Timeout);
        }
        let detector = self.detector.clone();
        let detecting = self.detecting.clone();
        let read = tokio::task::spawn_blocking(move || {
            let _in_flight = InFlight(detecting);
            detector.detect()
        });
        match tokio::time::timeout(DETECT_TIMEOUT, read).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(DetectionError::Unavailable),
            Err(_) => Err(DetectionError::Timeout),
        }
    }
    pub fn wake(&self, detect: bool) {
        if detect {
            self.detect_requested.store(true, Ordering::Release);
        }
        self.notify.notify_one();
    }
    /// Follow mode only records a desired zone; Runtime applies it at a protected boundary.
    pub fn observe(
        &self,
        detection: Result<String, DetectionError>,
        follow: bool,
        effective: &str,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        match detection {
            Ok(zone) => {
                // Newer detections replace older ones; returning to the effective zone clears it.
                state.pending = if follow && !same_zone(&zone, effective) {
                    Some(zone.clone())
                } else {
                    None
                };
                state.system = Some(zone);
                state.detection_error = None;
            }
            Err(error) => {
                // Keep the last valid effective zone and target; never fall back to UTC.
                state.detection_error = Some(error);
                if !follow {
                    state.pending = None;
                }
            }
        }
    }
    pub fn pending(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.pending.clone())
    }
    pub fn applied(&self, target: &str) {
        if let Ok(mut state) = self.state.lock() {
            if state.pending.as_deref() == Some(target) {
                state.pending = None;
            }
        }
    }
    /// A new target or a completed rebuild starts with a clean retry history.
    pub fn reset_rebuild(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.failures = 0;
            state.retry_at = 0;
            state.retry_wall = None;
            state.errors.clear();
        }
    }
    /// Only rebuild-owned jobs move the retry deadline; others are shown but keep their own policy.
    pub fn record_failure(&self, provider: &str, error: CoreError, cancelled: bool, owned: bool) {
        let now = self.now();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let error = mapping::error(error);
        state.errors.insert(provider.into(), error);
        if owned {
            state.failures = state.failures.saturating_add(1);
            let delay = retry_delay(state.failures, cancelled);
            state.retry_at = now.saturating_add(delay);
            state.retry_wall = wall_after(delay);
        }
    }
    pub fn clear_error(&self, provider: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.errors.remove(provider);
        }
    }
    /// A queued zone change or an active backoff defers the next rebuild-owned scan.
    pub fn rebuild_waiting(&self) -> bool {
        let now = self.now();
        match self.state.lock() {
            Ok(state) => state.pending.is_some() || now < state.retry_at,
            Err(_) => true,
        }
    }
    pub fn retry_allowed(&self, now: u64) -> bool {
        self.state.lock().is_ok_and(|state| now >= state.retry_at)
    }
    pub fn snapshot(&self) -> MonitorSnapshot {
        match self.state.lock() {
            Ok(state) => MonitorSnapshot {
                system: state.system.clone(),
                detection_error: state.detection_error.map(DetectionError::api),
                pending: state.pending.clone(),
                retry_at: state.retry_at,
                retry_wall: state.retry_wall.clone(),
                errors: state.errors.clone(),
            },
            Err(_) => MonitorSnapshot {
                system: None,
                detection_error: Some(DetectionError::Unavailable.api()),
                pending: None,
                retry_at: 0,
                retry_wall: None,
                errors: BTreeMap::new(),
            },
        }
    }
    pub async fn status_lock(&self) -> MutexGuard<'_, Published> {
        self.published.lock().await
    }
    pub fn emit(&self, status: api::TimezoneStatus) {
        (self.emit)(status);
    }
    pub async fn start(self: &Arc<Self>, runtime: &Arc<Runtime>) {
        let monitor = self.clone();
        let runtime = Arc::downgrade(runtime);
        *self.task.lock().await = Some(tokio::spawn(async move {
            monitor.run(runtime).await;
        }));
    }
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.notify.notify_one();
    }
    pub async fn stop(&self) {
        self.begin_stop();
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
    /// Native driver: no React timers. Startup, polling, resume, focus and job ends all land here.
    async fn run(&self, runtime: Weak<Runtime>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(TICK_SECONDS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_check: Option<u64> = None;
        let mut previous_mono = Instant::now();
        let mut previous_wall = chrono::Utc::now();
        loop {
            tokio::select! { _ = ticker.tick() => {}, _ = self.notify.notified() => {} }
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            let Some(runtime) = runtime.upgrade() else {
                break;
            };
            let mono = Instant::now();
            let wall = chrono::Utc::now();
            let woke = resumed(
                mono.duration_since(previous_mono).as_secs(),
                (wall - previous_wall).num_seconds(),
            );
            previous_mono = mono;
            previous_wall = wall;
            let now = self.now();
            let requested = self.detect_requested.swap(false, Ordering::AcqRel);
            if requested || woke || last_check.is_none_or(|at| now >= at + POLL_SECONDS) {
                last_check = Some(now);
                let detection = self.detect().await;
                runtime.observe_system_timezone(detection).await;
            }
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            runtime.drive_timezone().await;
        }
        self.stopping.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Offset, TimeZone};
    struct Fixed(Result<String, DetectionError>);
    impl SystemTimezone for Fixed {
        fn detect(&self) -> Result<String, DetectionError> {
            self.0.clone()
        }
    }
    struct Panics;
    impl SystemTimezone for Panics {
        fn detect(&self) -> Result<String, DetectionError> {
            panic!("synthetic detector failure")
        }
    }
    fn monitor(detector: Arc<dyn SystemTimezone>) -> Arc<TimezoneMonitor> {
        TimezoneMonitor::new(detector, Arc::new(|_| {}))
    }
    #[test]
    fn aliases_resolve_to_one_valid_idempotent_identity() {
        for (alias, target) in ALIASES {
            assert!(usage_core::application::validate_timezone(alias).is_ok());
            assert!(usage_core::application::validate_timezone(target).is_ok());
            assert_eq!(canonical(alias).as_deref(), Some(*target));
            assert_eq!(canonical(target).as_deref(), Some(*target));
        }
        assert_eq!(canonical("Asia/Calcutta").as_deref(), Some("Asia/Kolkata"));
        assert_eq!(canonical("Etc/UTC").as_deref(), Some("UTC"));
        assert_eq!(canonical("Europe/Oslo").as_deref(), Some("Europe/Oslo"));
        assert!(same_zone("US/Arizona", "America/Phoenix"));
        assert!(!same_zone("America/Phoenix", "America/Denver"));
        for invalid in ["", "Mars/Olympus", "utc", " UTC", "../../etc/passwd"] {
            assert!(canonical(invalid).is_none());
        }
    }
    fn offset(zone: &str, month: u32) -> i32 {
        let at = chrono::NaiveDate::from_ymd_opt(2026, month, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let zone: chrono_tz::Tz = zone.parse().unwrap();
        let offset = zone.offset_from_utc_datetime(&at).fix();
        offset.local_minus_utc()
    }
    #[test]
    fn identity_not_offset_decides_a_change() {
        // Los Angeles changes offset across DST but remains the same selection.
        assert_ne!(
            offset("America/Los_Angeles", 1),
            offset("America/Los_Angeles", 7)
        );
        let watcher = monitor(Arc::new(Fixed(Ok("America/Los_Angeles".into()))));
        watcher.observe(
            Ok("America/Los_Angeles".into()),
            true,
            "America/Los_Angeles",
        );
        assert!(watcher.pending().is_none());
        // Phoenix and Denver share a winter offset but are different selections.
        assert_eq!(offset("America/Phoenix", 1), offset("America/Denver", 1));
        watcher.observe(Ok("America/Denver".into()), true, "America/Phoenix");
        assert_eq!(watcher.pending().as_deref(), Some("America/Denver"));
        // An inconsistent alias of the effective zone does not request a rebuild.
        watcher.observe(Ok("Asia/Kolkata".into()), true, "Asia/Calcutta");
        assert!(watcher.pending().is_none());
    }
    #[test]
    fn fixed_mode_ignores_changes_and_failures_keep_the_last_valid_state() {
        let watcher = monitor(Arc::new(Fixed(Ok("UTC".into()))));
        watcher.observe(Ok("Asia/Tokyo".into()), false, "America/Phoenix");
        assert!(watcher.pending().is_none());
        assert_eq!(watcher.snapshot().system.as_deref(), Some("Asia/Tokyo"));
        watcher.observe(Ok("Asia/Tokyo".into()), true, "America/Phoenix");
        watcher.observe(Err(DetectionError::Invalid), true, "America/Phoenix");
        let snapshot = watcher.snapshot();
        assert_eq!(snapshot.pending.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(snapshot.system.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(
            snapshot.detection_error.map(|error| error.code),
            Some(api::ErrorCode::InvalidData)
        );
        watcher.observe(Err(DetectionError::Unavailable), false, "America/Phoenix");
        assert!(watcher.pending().is_none());
        watcher.observe(Ok("America/Phoenix".into()), true, "America/Phoenix");
        assert!(watcher.snapshot().detection_error.is_none());
    }
    #[test]
    fn rapid_detections_coalesce_to_the_latest_target() {
        let watcher = monitor(Arc::new(Fixed(Ok("UTC".into()))));
        for zone in ["Asia/Tokyo", "Europe/Paris", "America/Denver"] {
            watcher.observe(Ok(zone.into()), true, "America/Phoenix");
        }
        assert_eq!(watcher.pending().as_deref(), Some("America/Denver"));
        watcher.applied("Asia/Tokyo");
        assert_eq!(watcher.pending().as_deref(), Some("America/Denver"));
        watcher.applied("America/Denver");
        assert!(watcher.pending().is_none());
    }
    #[test]
    fn rebuild_retries_back_off_without_busy_loops() {
        assert_eq!(
            [1, 2, 3, 4, 5, 9].map(|failures| retry_delay(failures, false)),
            [60, 120, 240, 480, 900, 900]
        );
        assert_eq!(retry_delay(1, true), 300);
        assert_eq!(retry_delay(5, true), 900);
        let watcher = monitor(Arc::new(Fixed(Ok("UTC".into()))));
        watcher.record_failure("a", CoreError::CollectionFailed, false, false);
        assert!(watcher.retry_allowed(watcher.now()));
        watcher.record_failure("a", CoreError::CollectionFailed, false, true);
        let now = watcher.now();
        assert!(!watcher.retry_allowed(now));
        assert!(watcher.retry_allowed(now + 60));
        assert!(watcher.snapshot().retry_wall.is_some());
        watcher.reset_rebuild();
        assert!(watcher.retry_allowed(now));
        assert!(watcher.snapshot().errors.is_empty());
    }
    #[test]
    fn sleep_gaps_trigger_one_resume_check() {
        assert!(!resumed(TICK_SECONDS, TICK_SECONDS as i64));
        assert!(resumed(1, 3600));
        assert!(resumed(3600, 3600));
        assert!(!resumed(1, -3600));
    }
    #[test]
    fn sequence_strictly_increases_and_identical_content_is_not_reemitted() {
        let status = api::TimezoneStatus {
            api_version: api::API_VERSION.into(),
            sequence: String::new(),
            revision: "1".into(),
            mode: api::TimezoneMode::FollowSystem,
            effective_timezone: "UTC".into(),
            system_timezone: Some("UTC".into()),
            detection_error: None,
            pending_timezone: None,
            rebuild: api::TimezoneRebuildState::Idle,
            next_retry_at: None,
            providers: vec![],
        };
        let mut published = Published::default();
        assert!(published.changed(&status));
        assert!(!published.changed(&status));
        let first = published.next(status.clone());
        let second = published.next(status);
        assert_eq!(first.sequence, "1");
        assert_eq!(second.sequence, "2");
    }
    #[tokio::test]
    async fn failed_or_panicking_detection_is_reported_and_retried() {
        let watcher = monitor(Arc::new(Panics));
        assert_eq!(watcher.detect().await, Err(DetectionError::Unavailable));
        assert_eq!(watcher.detect().await, Err(DetectionError::Unavailable));
        let watcher = monitor(Arc::new(Fixed(Ok("Asia/Tokyo".into()))));
        assert_eq!(watcher.detect().await.as_deref(), Ok("Asia/Tokyo"));
        let detected = OsTimezone.detect().unwrap();
        assert_eq!(canonical(&detected), Some(detected));
    }
}
