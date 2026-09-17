//! The machine-wide limit on how many tasks of one concurrency group run
//! at once, configured by
//! [`concurrencyGroups`](Config::concurrency_groups) and a task's
//! `concurrencyGroup`.
//!
//! A pool of `N` slot files lives under the state directory, one pool per
//! group. A task holds an exclusive advisory lock on one slot file for as
//! long as its scripts run. The operating system drops the lock when the
//! process ends, however it ends, so a crashed or killed holder never
//! leaves a stale slot behind. Each process honours its own configured
//! limit, so two workspaces on one pool with different limits reach into
//! it as far as their own setting allows.
//!
//! A holder stamps the group into [`HELD_CONCURRENCY_GROUPS_ENV`] for the
//! scripts it spawns. A nested `pnpm run` that finds a task's group there
//! runs under the slot its parent holds, which is what keeps a script that
//! calls `pnpm run` from waiting on itself.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

/// Env var that carries the groups the parent invocations hold slots of,
/// comma-separated.
pub(crate) const HELD_CONCURRENCY_GROUPS_ENV: &str = "PNPM_HELD_CONCURRENCY_GROUPS";

const FIRST_POLL: Duration = Duration::from_millis(100);
const MAX_POLL: Duration = Duration::from_secs(1);
const WAIT_NOTICE_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to take a slot of concurrency group {group:?} in {}", pool.display())]
#[diagnostic(code(ERR_PNPM_CONCURRENCY_GROUP_SLOT_FAILED))]
pub(crate) struct ConcurrencyGroupSlotError {
    group: String,
    pool: PathBuf,
    #[error(source)]
    source: io::Error,
}

/// One held slot. Dropping it, or the process ending, frees the slot.
pub(crate) struct ConcurrencyGroupSlot {
    group: String,
    _file: File,
}

impl ConcurrencyGroupSlot {
    pub(crate) fn group(&self) -> &str {
        &self.group
    }
}

/// Take a slot of the group the task named `script` belongs to, waiting
/// for one to free up when they are all held. `None` when the task names
/// no group, the group has no limit, or a parent invocation already holds
/// a slot of the group.
///
/// `emit` receives a notice when the wait starts and every
/// [`WAIT_NOTICE_EVERY`] after, naming who holds the slots.
pub(crate) fn acquire_concurrency_group_slot(
    config: &Config,
    script: &str,
    emit: fn(&LogEvent),
) -> Result<Option<ConcurrencyGroupSlot>, ConcurrencyGroupSlotError> {
    let Some(group) =
        config.tasks.get(script).and_then(|task| task.concurrency_group.as_deref())
    else {
        return Ok(None);
    };
    let Some(limit) = config.concurrency_groups
        .get(group)
        .copied()
        .filter(|limit| *limit > 0)
    else {
        return Ok(None);
    };
    if held_groups(std::env::var(HELD_CONCURRENCY_GROUPS_ENV).ok().as_deref())
        .any(|held| held == group)
    {
        return Ok(None);
    }
    let pool = SlotPool { dir: config.state_dir.join("run-slots").join(group), limit };
    pool.acquire(|| {
        let holders = pool.holders();
        emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: format!(
                r#"Waiting to run "{script}": all {limit} slots of concurrency group "{group}" are held ({}). Holders: {}"#,
                pool.dir.display(),
                if holders.is_empty() { "unknown".to_string() } else { holders.join("; ") },
            ),
        }));
    })
    .map(|file| Some(ConcurrencyGroupSlot { group: group.to_string(), _file: file }))
    .map_err(|source| ConcurrencyGroupSlotError {
        group: group.to_string(),
        pool: pool.dir.clone(),
        source,
    })
}

/// `extra_env` with `group` added to the held groups the spawned script
/// sees, on top of the ones this process inherited.
pub(crate) fn with_held_group(
    extra_env: &HashMap<String, String>,
    group: &str,
) -> HashMap<String, String> {
    let inherited = std::env::var(HELD_CONCURRENCY_GROUPS_ENV).ok();
    let held: Vec<&str> = held_groups(inherited.as_deref())
        .filter(|held| *held != group)
        .chain(std::iter::once(group))
        .collect();
    let mut extra_env = extra_env.clone();
    extra_env.insert(HELD_CONCURRENCY_GROUPS_ENV.to_string(), held.join(","));
    extra_env
}

fn held_groups(value: Option<&str>) -> impl Iterator<Item = &str> {
    value
        .unwrap_or_default()
        .split(',')
        .filter(|group| !group.is_empty())
}

struct SlotPool {
    dir: PathBuf,
    limit: u32,
}

impl SlotPool {
    fn acquire(&self, mut on_wait: impl FnMut()) -> io::Result<File> {
        fs::create_dir_all(&self.dir)?;
        let mut poll = FIRST_POLL;
        let mut last_notice: Option<Instant> = None;
        loop {
            if let Some(file) = self.try_acquire()? {
                return Ok(file);
            }
            if last_notice.is_none_or(|at| at.elapsed() >= WAIT_NOTICE_EVERY) {
                on_wait();
                last_notice = Some(Instant::now());
            }
            std::thread::sleep(poll);
            poll = (poll * 2).min(MAX_POLL);
        }
    }

    fn try_acquire(&self) -> io::Result<Option<File>> {
        for index in 0..self.limit {
            let file = self.open_slot(index)?;
            match file.try_lock() {
                Ok(()) => {
                    // Best effort: the stamp only feeds the waiting notice.
                    let _ = self.write_holder(index);
                    return Ok(Some(file));
                }
                Err(std::fs::TryLockError::WouldBlock) => continue,
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        Ok(None)
    }

    fn open_slot(&self, index: u32) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(index.to_string()))
    }

    /// The holder stamp lives beside the slot rather than in it: a held
    /// slot cannot be read on the platforms where the lock is mandatory.
    fn holder_path(&self, index: u32) -> PathBuf {
        self.dir.join(format!("{index}.holder"))
    }

    fn write_holder(&self, index: u32) -> io::Result<()> {
        let cwd = std::env::current_dir().unwrap_or_default();
        fs::write(
            self.holder_path(index),
            format!("pid {} in {}", std::process::id(), cwd.display()),
        )
    }

    /// The stamps of the slots that are held right now. A slot whose lock
    /// this probe can take is free, and its stale stamp is left out.
    fn holders(&self) -> Vec<String> {
        (0..self.limit)
            .filter_map(|index| {
                let file = self.open_slot(index).ok()?;
                match file.try_lock() {
                    Err(std::fs::TryLockError::WouldBlock) => {}
                    Ok(()) | Err(std::fs::TryLockError::Error(_)) => return None,
                }
                let holder = fs::read_to_string(self.holder_path(index)).ok()?;
                let holder = holder.trim();
                (!holder.is_empty()).then(|| holder.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
