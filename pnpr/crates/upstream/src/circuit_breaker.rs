use super::{Duration, Instant, Mutex};

/// Verdaccio's `max_fails` / `fail_timeout` circuit breaker. After
/// `max_fails` consecutive failures the upstream is considered down and
/// requests short-circuit, until `fail_timeout` elapses since the last
/// failure — a single probe is then allowed through, and its success
/// resets the breaker or its failure restarts the cooldown.
///
/// The half-open probe is gated by the cooldown window itself: admitting
/// a probe advances `last_failure` to now, so concurrent callers in that
/// window stay short-circuited and a recovering upstream sees one probe
/// rather than a stampede. Crucially this can't *stick* — a probe whose
/// request is cancelled or dropped before it reports back simply lets the
/// window lapse, after which the next caller probes. A sticky in-flight
/// flag would deadlock the breaker on a cancelled request.
///
/// The counter and the timestamp live behind one [`Mutex`] so a threshold
/// check and its timestamp can never be observed half-updated. The lock
/// is held only for trivial field reads/writes (never across the network
/// request), so contention is negligible; a poisoned lock is recovered
/// rather than propagated as a panic, keeping a failing upstream from
/// taking the registry down.
#[derive(Debug)]
pub(super) struct CircuitBreaker {
    pub(super) max_fails: u32,
    pub(super) fail_timeout: Duration,
    pub(super) state: Mutex<BreakerState>,
}

#[derive(Debug, Default)]
pub(super) struct BreakerState {
    pub(super) failed_requests: u32,
    pub(super) last_failure: Option<Instant>,
}

impl CircuitBreaker {
    pub(super) fn new(max_fails: u32, fail_timeout: Duration) -> Self {
        Self { max_fails, fail_timeout, state: Mutex::new(BreakerState::default()) }
    }

    /// Recover the guard from a poisoned lock instead of panicking: the
    /// breaker only ever holds plain counters, so the worst a poisoned
    /// guard carries is a stale failure count, never an invariant break.
    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, BreakerState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Try to admit one request. Returns `true` while under `max_fails`
    /// consecutive failures (or when the breaker is disabled). Once the
    /// `fail_timeout` cooldown has elapsed it admits one probe per window:
    /// the admitting caller advances the cooldown, so concurrent callers
    /// stay short-circuited until the next window opens. `max_fails == 0`
    /// disables the breaker entirely.
    pub(super) fn try_acquire(&self) -> bool {
        let mut state = self.lock();
        if self.max_fails == 0 || state.failed_requests < self.max_fails {
            return true;
        }
        // Tripped: stay open until the cooldown since the last failure
        // lapses. (`failed_requests >= max_fails` always implies a
        // recorded `last_failure`, so the `None` arm is unreachable; it
        // fails open for safety.)
        let cooled_down = state.last_failure.is_none_or(|at| at.elapsed() >= self.fail_timeout);
        if !cooled_down {
            return false;
        }
        // Admit this probe and re-arm the cooldown so the next caller is
        // held back for another `fail_timeout`. If the probe never reports
        // back (cancelled mid-request), the window simply lapses and the
        // following caller probes — the breaker can't deadlock.
        state.last_failure = Some(Instant::now());
        true
    }

    pub(super) fn record_success(&self) {
        *self.lock() = BreakerState::default();
    }

    pub(super) fn record_failure(&self) {
        let mut state = self.lock();
        state.failed_requests = state.failed_requests.saturating_add(1);
        state.last_failure = Some(Instant::now());
    }
}
