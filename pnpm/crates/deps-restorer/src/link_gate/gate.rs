use super::climb::{Climb, INITIAL_PERMITS, MAX_PERMITS, MIN_PERMITS};
use pnpm_config::Config;
use pnpm_store_dir::StoreDir;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Condvar, LazyLock, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

/// Pins the gate to this many concurrent calls and turns the climb off.
/// Unset, `0`, or a value that is not a number leaves the climb on.
const PINNED_PERMITS_ENV: &str = "PNPM_MACOS_LINK_CONCURRENCY";

/// A window closes once this many calls completed in it and the gate
/// had calls in flight for [`WINDOW_MIN_BUSY`]. The count keeps a
/// window above the noise of a few slow calls; the duration keeps the
/// windows of a cold install, whose per-file calls complete tens per
/// millisecond, long enough for a width change to show in the rate.
const WINDOW_CALLS: u64 = 256;
const WINDOW_MIN_BUSY: Duration = Duration::from_millis(50);

/// An install that completed fewer calls than this did not run the
/// climb long enough for its width to be worth seeding the next one.
const SAVE_MIN_CALLS: u64 = 2048;

static GATE: LazyLock<Gate> = LazyLock::new(Gate::from_env);

struct Gate {
    state: Mutex<State>,
    released: Condvar,
}

struct State {
    climb: Climb,
    pinned: Option<usize>,
    /// The device [`load`] last tuned the climb for.
    device: Option<u64>,
    in_flight: usize,
    /// Calls completed since the climb was tuned for [`Self::device`].
    calls_on_device: u64,
    window: Window,
}

impl State {
    fn permits(&self) -> usize {
        self.pinned.unwrap_or_else(|| self.climb.permits())
    }
}

/// One measurement of the completion rate.
///
/// The rate is taken over the time the gate had calls in flight, so the
/// planning and fetching between link batches do not count as slow
/// linking. A window in which no caller waited is not observed: the
/// width did not limit it.
#[derive(Default)]
struct Window {
    calls: u64,
    busy: Duration,
    busy_since: Option<Instant>,
    waited: bool,
}

impl Window {
    fn busy(&self) -> Duration {
        self.busy + self.busy_since.map_or(Duration::ZERO, |since| since.elapsed())
    }
}

impl Gate {
    fn from_env() -> Self {
        let pinned = std::env::var(PINNED_PERMITS_ENV)
            .ok()
            .and_then(|permits| permits.trim().parse::<usize>().ok())
            .filter(|permits| *permits > 0);
        Self {
            state: Mutex::new(State {
                climb: Climb::new(INITIAL_PERMITS),
                pinned,
                device: None,
                in_flight: 0,
                calls_on_device: 0,
                window: Window::default(),
            }),
            released: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn acquire(&self) {
        let mut state = self.lock();
        while state.in_flight >= state.permits() {
            state.window.waited = true;
            state = self.released.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        if state.in_flight == 0 {
            state.window.busy_since = Some(Instant::now());
        }
        state.in_flight += 1;
    }

    fn release(&self) {
        let mut state = self.lock();
        state.in_flight -= 1;
        state.calls_on_device += 1;
        state.window.calls += 1;
        if state.in_flight == 0
            && let Some(since) = state.window.busy_since.take()
        {
            state.window.busy += since.elapsed();
        }
        let busy = state.window.busy();
        let widened = state.pinned.is_none()
            && state.window.calls >= WINDOW_CALLS
            && busy >= WINDOW_MIN_BUSY
            && close_window(&mut state, busy);
        drop(state);
        // A wider gate admits waiters beyond the one a single release
        // would wake.
        if widened {
            self.released.notify_all();
        } else {
            self.released.notify_one();
        }
    }
}

/// Feed the window's rate to the climb if a caller waited in it, and
/// start the next window. Returns whether the gate widened.
fn close_window(state: &mut State, busy: Duration) -> bool {
    let window = std::mem::take(&mut state.window);
    if state.in_flight > 0 {
        state.window.busy_since = Some(Instant::now());
    }
    if !window.waited {
        return false;
    }
    let before = state.climb.permits();
    let calls_per_second = (window.calls as f64 / busy.as_secs_f64()) as u64;
    state.climb.observe(calls_per_second) > before
}

/// Releases its permit when dropped, so a panicking call does not shrink
/// the gate for the rest of the process.
struct Permit;

impl Drop for Permit {
    fn drop(&mut self) {
        GATE.release();
    }
}

/// Run `syscall` once the gate admits it.
///
/// `syscall` must not itself call [`gated`]: a permit held across a
/// nested wait could leave every permit waiting on another.
pub(crate) fn gated<Output>(syscall: impl FnOnce() -> Output) -> Output {
    GATE.acquire();
    let _permit = Permit;
    syscall()
}

/// Tune the gate for the volume holding `destination`, starting from the
/// width a previous install into it settled on. A gate already tuned for
/// that volume in this process keeps its state, and a pinned gate keeps
/// its pin.
pub(crate) fn load(store_dir: &StoreDir, destination: &Path) {
    let Some(device) = device_of(destination) else {
        return;
    };
    {
        let state = GATE.lock();
        if state.pinned.is_some() || state.device == Some(device) {
            return;
        }
    }
    let climb = fs::read_to_string(record_path(store_dir, device))
        .ok()
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|permits| (MIN_PERMITS..=MAX_PERMITS).contains(permits))
        .map_or_else(|| Climb::new(INITIAL_PERMITS), Climb::seeded);
    let mut state = GATE.lock();
    state.climb = climb;
    state.device = Some(device);
    state.calls_on_device = 0;
    state.window =
        Window { busy_since: (state.in_flight > 0).then(Instant::now), ..Window::default() };
}

/// Record the width the gate settled on for the volume holding
/// `destination`, so the next install into it starts there. A frozen
/// store is left untouched.
pub(crate) fn save(config: &Config, destination: &Path) {
    if config.frozen_store {
        return;
    }
    let Some(device) = device_of(destination) else {
        return;
    };
    let (permits, calls) = {
        let state = GATE.lock();
        if state.pinned.is_some() || state.device != Some(device) {
            return;
        }
        (state.climb.permits(), state.calls_on_device)
    };
    tracing::debug!(
        target: "pacquet::link_gate",
        permits,
        calls,
        "link gate width after the install",
    );
    if calls < SAVE_MIN_CALLS {
        return;
    }
    let path = record_path(&config.store_dir, device);
    if let Err(error) = pnpm_fs::write_atomic(&path, format!("{permits}\n").as_bytes()) {
        tracing::debug!(
            target: "pacquet::link_gate",
            path = %path.display(),
            %error,
            "failed to record the link gate width",
        );
    }
}

/// The device of `destination`'s nearest existing ancestor. The knee is
/// the volume's, so records are keyed by it.
fn device_of(destination: &Path) -> Option<u64> {
    fs::metadata(crate::dir_clone_cache::deepest_existing_ancestor(destination))
        .ok()
        .map(|metadata| metadata.dev())
}

fn record_path(store_dir: &StoreDir, device: u64) -> PathBuf {
    store_dir.link_concurrency().join(device.to_string())
}
