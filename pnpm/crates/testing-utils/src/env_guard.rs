//! Test-only helper: serialize env-var mutations across parallel
//! tests, and snapshot/restore selected variables around each block.
//!
//! `env::set_var` / `env::remove_var` are process-global and outlive
//! any `#[test]` they were set inside. Rust / nextest run unit tests in
//! parallel threads inside the same process by default, so two
//! concurrent tests mutating the same variable can easily observe each
//! other's half-set state. This module guards both concerns:
//!
//! * a process-wide `Mutex` is acquired for the lifetime of each
//!   guard, so only one env-mutating test holds the "env-is-mine" lock
//!   at a time;
//! * the snapshot/restore pass on `Drop` puts the named variables back
//!   to whatever the caller inherited, so the guard can't leak state
//!   into unrelated tests or stomp a developer's shell.
//!
//! This is the sanctioned way for a test to write the environment, and
//! `clippy.toml` leaves no other. Prefer not to: a subject that takes
//! the value as a parameter, or through an injected capability, needs
//! neither this guard nor its lock.

use std::{
    env,
    ffi::{OsStr, OsString},
    sync::{LazyLock, Mutex, MutexGuard},
};

/// Serialization mutex for env-mutating tests. A single `Mutex<()>` —
/// uncontended outside the handful of tests that need it, cheap when
/// held.
fn env_mutex() -> &'static Mutex<()> {
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    &LOCK
}

/// Restore a named set of env vars on drop and hold the env-mutation
/// lock for that lifetime.
#[must_use = "the guard must be held until the test ends"]
pub struct EnvGuard {
    // `OsString` so non-UTF-8 values round-trip. `env::var` returns
    // `Err(NotUnicode)` for those and would silently coerce to "absent"
    // here — then `Drop` would `remove_var` a variable the user
    // actually had set, clobbering CI / shell state. `env::var_os` +
    // `OsString` preserves the raw bytes.
    saved: Vec<(&'static str, Option<OsString>)>,
    // Released on drop, last. Ignore the poison case: if another
    // env-mutating test panicked while holding the lock, the env vars
    // it touched were restored by *its* guard's `Drop` before the
    // unwind propagated, so the environment is in a known state and
    // the next test can safely proceed.
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    /// Acquire the process-wide env-mutation lock and snapshot the
    /// current values of `vars`. When the returned guard drops, each
    /// variable is put back to exactly what it was (set to the recorded
    /// value, or removed if it was absent), and the lock is released.
    pub fn snapshot<Iter>(vars: Iter) -> Self
    where
        Iter: IntoIterator<Item = &'static str>,
    {
        let lock = env_mutex().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved = vars
            .into_iter()
            .map(|name| (name, env::var_os(name)))
            .collect();
        EnvGuard { saved, _lock: lock }
    }

    /// Like [`Self::snapshot`], but also unsets every named variable,
    /// so the test sees only what it sets itself and never what the
    /// developer's shell or CI exports.
    pub fn scrubbed<Iter>(vars: Iter) -> Self
    where
        Iter: IntoIterator<Item = &'static str>,
    {
        let guard = Self::snapshot(vars);
        for (name, _) in &guard.saved {
            guard.remove(name);
        }
        guard
    }

    /// Set a variable under the lock the guard holds.
    ///
    /// The variable must have been listed in [`Self::snapshot`]'s
    /// `vars` so the `Drop` impl will restore it — otherwise the
    /// mutation leaks beyond the guard. We accept this footgun rather
    /// than re-snapshotting on every `set` because the snapshot-then-
    /// set pattern matches every call site, and the test fixtures
    /// that need it are small.
    pub fn set(&self, name: &str, value: impl AsRef<OsStr>) {
        #[expect(clippy::disallowed_methods, reason = "the helper the lint points to")]
        // SAFETY: the guard holds `env_mutex`, so no other
        // `EnvGuard`-using test in this process is running
        // concurrently. The variable was named at `snapshot` time,
        // so `Drop` will restore it.
        unsafe {
            env::set_var(name, value);
        }
    }

    /// Unset a variable under the lock the guard holds. Carries the
    /// same obligation as [`Self::set`]: the variable must have been
    /// named at [`Self::snapshot`] time.
    pub fn remove(&self, name: &str) {
        #[expect(clippy::disallowed_methods, reason = "the helper the lint points to")]
        // SAFETY: the guard holds `env_mutex`, so no other
        // `EnvGuard`-using test in this process is running
        // concurrently, and the variable was named at `snapshot` time
        // so `Drop` restores it.
        unsafe {
            env::remove_var(name);
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, prior) in &self.saved {
            #[expect(clippy::disallowed_methods, reason = "the helper the lint points to")]
            // SAFETY: the `_lock` field is still held for the duration of this
            // `Drop`, so no other env-mutating test in this process is running
            // concurrently. The variables themselves are only read by tests
            // that take the same lock.
            unsafe {
                match prior {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }
}
