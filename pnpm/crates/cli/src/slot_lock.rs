//! The lock over a slot of a shared global virtual store that a pinned
//! package manager or runtime is installed into.
//!
//! Materializing a slot is destructive — one found carrying an
//! interrupted-build marker is removed and re-staged — so only the
//! process holding the slot's lock enters it. A process that finds the
//! lock held waits only briefly. A lock whose holder has died is taken
//! over at once, so a held lock is a live install, and one that has not
//! finished within the wait may take longer still than installing again.
//! Rather than wait it out, or enter the slot unlocked, the process
//! installs into a [`pnpm_store_dir::PrivateInstall`] of its own.

use pnpm_fs::DirLock;
use std::{io, path::PathBuf, time::Duration};

/// How long a process waits for a held slot lock before installing
/// privately.
const WAIT: Duration = Duration::from_secs(5);

/// Comfortably above how long a slot install can legitimately take, so a
/// holder that cannot prove it is alive — an older pnpm, or one on a
/// filesystem that cannot hold an OS file lock — never has its lock
/// stolen.
const ABANDONED_AFTER: Duration = Duration::from_mins(30);

/// Take the slot lock at `path`. `Ok(None)` is a lock another process
/// holds; the caller installs privately instead of waiting further.
pub(crate) fn acquire(path: PathBuf) -> io::Result<Option<DirLock>> {
    DirLock::acquire(path, WAIT, ABANDONED_AFTER)
}
