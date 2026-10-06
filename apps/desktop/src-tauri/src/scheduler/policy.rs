//! Pure policy; all times are monotonic seconds supplied by the driver/tests.
#[derive(Default)]
pub(super) struct Policy {
    pub baseline: Option<String>,
    pub next_check: u64,
    pub pending: Option<u64>,
    pub forced: bool,
    pub generation: u64,
    pub last_started: Option<u64>,
    pub retry_at: u64,
    pub failures: u32,
}
impl Policy {
    pub fn new(now: u64) -> Self {
        Self {
            pending: Some(now + 2),
            next_check: now + 2,
            ..Self::default()
        }
    }
    pub fn request(&mut self, now: u64, force: bool) {
        self.pending.get_or_insert(now + 2);
        if force {
            self.forced = true;
            self.generation = self.generation.saturating_add(1);
        }
    }
    pub fn due(&self, now: u64) -> bool {
        now >= self.retry_at && (self.pending.is_some_and(|at| now >= at) || now >= self.next_check)
    }
    pub fn checked(&mut self, now: u64, interval: u64) {
        self.pending = None;
        self.next_check = now + interval;
    }
    pub fn changed(&self, observation: &str) -> bool {
        self.forced || self.baseline.as_deref() != Some(observation)
    }
    pub fn scan_allowed(&self, now: u64, interval: u64) -> bool {
        now >= self.retry_at && self.last_started.is_none_or(|at| now >= at + interval)
    }
    pub fn started(&mut self, now: u64) -> u64 {
        self.last_started = Some(now);
        self.generation
    }
    pub fn succeeded(&mut self, before: String, generation: u64) {
        self.baseline = Some(before);
        self.failures = 0;
        self.retry_at = 0;
        if self.generation == generation {
            self.forced = false;
        }
    }
    pub fn failed(&mut self, now: u64, interval: u64) {
        self.failures = self.failures.saturating_add(1);
        let backoff = 30u64.saturating_mul(1u64 << self.failures.min(6)).min(900);
        self.retry_at = now + interval.max(backoff);
        self.pending = None;
        self.next_check = self.retry_at;
    }
    pub fn cancelled(&mut self, now: u64, interval: u64) {
        self.pending = None;
        self.retry_at = now + interval;
        self.next_check = self.retry_at;
    }
    pub fn deadline(&self) -> u64 {
        self.pending.unwrap_or(self.next_check).max(self.retry_at)
    }
}
/// Both Windows and macOS may exclude sleep from Instant. Wall clock is a wake hint only;
/// debounce/rate limits/retry deadlines always use the supplied monotonic clock.
pub(super) fn resumed(monotonic_gap: u64, wall_gap: i64) -> bool {
    monotonic_gap > 10 || wall_gap > 10
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_inspection_does_not_consume_the_minimum_scan_interval() {
        let mut p = Policy::new(0);
        p.checked(29, 60);
        let generation = p.started(31);
        p.succeeded("before".into(), generation);
        p.request(32, true);
        assert!(!p.scan_allowed(60, 60));
        assert!(!p.scan_allowed(90, 60));
        assert!(p.scan_allowed(91, 60));
    }
    #[test]
    fn bursts_first_enable_and_continuous_writes_are_bounded() {
        let mut p = Policy::new(0);
        p.next_check = 2;
        for _ in 0..10_000 {
            p.request(0, true);
        }
        assert!(!p.due(1));
        assert!(p.due(2));
        assert!(p.changed("a"));
        p.checked(2, 60);
        let generation = p.started(2);
        p.succeeded("a".into(), generation);
        assert!(!p.changed("a"));
        p.request(3, true);
        assert!(!p.scan_allowed(5, 60));
        assert!(p.scan_allowed(62, 60));
    }
    #[test]
    fn events_during_scan_and_same_metadata_content_hints_survive() {
        let mut p = Policy::new(0);
        let generation = p.started(2);
        p.request(3, true);
        p.succeeded("before".into(), generation);
        assert!(p.changed("before"));
        assert!(p.changed("after"));
    }
    #[test]
    fn failure_and_cancel_never_acknowledge_inputs_or_busy_loop() {
        let mut p = Policy::new(0);
        p.baseline = Some("old".into());
        p.failed(5, 60);
        assert!(!p.due(6));
        assert_eq!(p.baseline.as_deref(), Some("old"));
        for _ in 0..50 {
            p.failed(10, 60);
        }
        assert_eq!(p.retry_at, 910);
        p.cancelled(20, 60);
        p.request(21, true);
        assert!(!p.due(79));
        assert!(p.due(80));
    }
    #[test]
    fn sleep_and_clock_changes_produce_one_pending_check() {
        assert!(resumed(1, 3600));
        assert!(resumed(3600, 3600));
        assert!(!resumed(1, -3600));
        let mut p = Policy::new(0);
        p.checked(2, 60);
        p.request(3600, false);
        p.request(3600, false);
        assert_eq!(p.pending, Some(3602));
        p.checked(3602, 60);
        assert!(!p.due(3603));
    }
}
