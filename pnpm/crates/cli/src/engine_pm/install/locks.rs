//! The host-wide locks an engine install takes: one around the
//! package-manager environment, and one around each engine's slot.

use crate::slot_lock;
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_fs::DirLock;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use std::{path::Path, time::Duration};

/// Take the host-wide lock guarding this engine's slot, or `None` when
/// another process holds it: the engine is then installed privately. A
/// lock that cannot be established at all is reported and treated the
/// same, since the private install needs none.
pub(super) fn engine_install_lock<Reporter: self::Reporter>(
    config: &Config,
    package_name: &str,
    version: &str,
) -> Option<DirLock> {
    let name = format!("{}@{version}.lock", package_name.replace('/', "+"));
    let path = config.store_dir
        .tmp()
        .join("engine-locks")
        .join(name);
    match slot_lock::acquire(path.clone()) {
        Ok(lock) => lock,
        Err(error) => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Could not lock the package manager engine install at {}: {error}. Installing it into a private directory instead.",
                    path.display(),
                ),
                prefix: String::new(),
            }));
            None
        }
    }
}

pub(super) async fn package_manager_env_lock<Reporter: self::Reporter>(
    config: &Config,
) -> miette::Result<Option<DirLock>> {
    let path = config.store_dir.tmp().join("engine-locks/package-manager-env.lock");
    tokio::task::spawn_blocking(move || {
        acquire_install_lock::<Reporter>(&path, "the package-manager environment")
    })
    .await
    .into_diagnostic()
    .wrap_err("wait for the package-manager environment lock")
}

fn acquire_install_lock<Reporter: self::Reporter>(path: &Path, subject: &str) -> Option<DirLock> {
    /// Long enough for a cold install of the engine over a slow link,
    /// short enough that a wedged host isn't hung on indefinitely.
    const WAIT: Duration = Duration::from_mins(5);
    /// Comfortably above `WAIT`, so a process still legitimately
    /// installing never has its lock stolen by one that gave up waiting.
    const ABANDONED_AFTER: Duration = Duration::from_mins(30);

    let error = match DirLock::acquire(path.to_path_buf(), WAIT, ABANDONED_AFTER) {
        Ok(lock) => return lock,
        Err(error) => error,
    };
    // Through the reporter rather than `tracing`, which is only wired to
    // a subscriber when `TRACE` is set and would drop this on every
    // ordinary run.
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            "Could not lock {subject} at {}: {error}. Installing without it, which is unsafe if another pnpm is installing concurrently.",
            path.display(),
        ),
        prefix: String::new(),
    }));
    None
}
