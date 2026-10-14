use super::climb::{Climb, INITIAL_PERMITS, MAX_PERMITS, MIN_PERMITS};
use pnpm_store_dir::StoreDir;
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Condvar, LazyLock, Mutex},
    time::{Duration, Instant},
};

/// Pins the gate to this many concurrent calls and turns the climb off.
/// Unset, `0`, or a value that is not a number leaves the climb on.
const PINNED_PERMITS_ENV: &str = "PNPM_MACOS_LINK_CONCURRENCY";

/// A window closes once this many calls completed in it and
/// [`WINDOW_MIN_DURATION`] passed. The count keeps a window above the
/// noise of a few slow calls; the duration keeps the windows of a
/// cold install, whose per-file calls complete tens per millisecond,
/// long enough for a width change to show in the rate.
const WINDOW_CALLS: u64 = 256;
const WINDOW_MIN_DURATION: Duration = Duration::from_millis(50);

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
    pinned: bool,
    in_flight: usize,
    total_calls: u64,
    window: Window,
}

struct Window {
    opened: Instant,
    calls: u64,
}

impl Gate {
    fn from_env() -> Self {
        let pinned = std::env::var(PINNED_PERMITS_ENV)
            .ok()
            .and_then(|permits| permits.trim().parse::<usize>().ok())
            .filter(|permits| *permits > 0);
        Self {
            state: Mutex::new(State {
                climb: Climb::new(pinned.unwrap_or(INITIAL_PERMITS)),
                pinned: pinned.is_some(),
                in_flight: 0,
                total_calls: 0,
                window: Window { opened: Instant::now(), calls: 0 },
            }),
            released: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn acquire(&self) {
        let mut state = self.lock();
        while state.in_flight >= state.climb.permits() {
            state = self.released.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.in_flight += 1;
    }

    fn release(&self) {
        let mut state = self.lock();
        state.in_flight -= 1;
        state.total_calls += 1;
        state.window.calls += 1;
        let elapsed = state.window.opened.elapsed();
        if state.pinned || state.window.calls < WINDOW_CALLS || elapsed < WINDOW_MIN_DURATION {
            drop(state);
            self.released.notify_one();
            return;
        }
        let calls_per_second = (state.window.calls as f64 / elapsed.as_secs_f64()) as u64;
        let before = state.climb.permits();
        let after = state.climb.observe(calls_per_second);
        state.window = Window { opened: Instant::now(), calls: 0 };
        drop(state);
        // A wider gate admits waiters beyond the one a single release
        // would wake.
        if after > before {
            self.released.notify_all();
        } else {
            self.released.notify_one();
        }
    }
}

/// Run `syscall` once the gate admits it.
///
/// `syscall` must not itself call [`gated`]: a permit held across a
/// nested wait could leave every permit waiting on another.
pub(crate) fn gated<Output>(syscall: impl FnOnce() -> Output) -> Output {
    GATE.acquire();
    let output = syscall();
    GATE.release();
    output
}

/// Seed the gate with the width a previous install into the volume
/// holding `destination` settled on. A pinned gate keeps its pin.
pub(crate) fn load(store_dir: &StoreDir, destination: &Path) {
    let Some(path) = record_path(store_dir, destination) else {
        return;
    };
    let Some(permits) = fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|permits| (MIN_PERMITS..=MAX_PERMITS).contains(permits))
    else {
        return;
    };
    let mut state = GATE.lock();
    if state.pinned || state.total_calls > 0 {
        return;
    }
    state.climb = Climb::seeded(permits);
}

/// Record the width the gate settled on for the volume holding
/// `destination`, so the next install into it starts there.
pub(crate) fn save(store_dir: &StoreDir, destination: &Path) {
    let (permits, total_calls) = {
        let state = GATE.lock();
        if state.pinned {
            return;
        }
        (state.climb.permits(), state.total_calls)
    };
    tracing::debug!(
        target: "pacquet::link_gate",
        permits,
        calls = total_calls,
        "link gate width after the install",
    );
    if total_calls < SAVE_MIN_CALLS {
        return;
    }
    let Some(path) = record_path(store_dir, destination) else {
        return;
    };
    if let Err(error) = write_record(&path, permits) {
        tracing::debug!(
            target: "pacquet::link_gate",
            path = %path.display(),
            %error,
            "failed to record the link gate width",
        );
    }
}

fn write_record(path: &Path, permits: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{permits}\n"))
}

/// `<store>/link-concurrency/<device>`, keyed by the device the
/// destination's nearest existing ancestor sits on, since the knee is
/// the volume's.
fn record_path(store_dir: &StoreDir, destination: &Path) -> Option<PathBuf> {
    let device =
        fs::metadata(crate::dir_clone_cache::deepest_existing_ancestor(destination)).ok()?.dev();
    Some(store_dir.link_concurrency().join(device.to_string()))
}
