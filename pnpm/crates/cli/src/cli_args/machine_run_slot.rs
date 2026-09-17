//! The machine-wide cap on concurrent `pnpm run` / `pnpm exec` invocations
//! that [`Config::machine_run_concurrency`] configures.
//!
//! A pool of `N` slot files lives under the state directory, one pool per
//! [`Config::machine_run_concurrency_group`]. An invocation holds an
//! exclusive advisory lock on one slot file for as long as its scripts run.
//! The operating system drops the lock when the process ends, however it
//! ends, so a crashed or killed holder never leaves a stale slot behind.
//! Each process honours its own configured limit, so two workspaces on one
//! pool with different limits reach into it as far as their own setting
//! allows.
//!
//! A holder stamps [`MACHINE_RUN_SLOT_ENV`] into the environment of every
//! script and command it spawns. A nested invocation that finds its own
//! group there runs under the slot its parent already holds, which is what
//! keeps a script that calls `pnpm run` from waiting on itself.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

/// Env var that carries the group of the slot a parent invocation holds.
pub(crate) const MACHINE_RUN_SLOT_ENV: &str = "PNPM_MACHINE_RUN_SLOT_GROUP";

const FIRST_POLL: Duration = Duration::from_millis(100);
const MAX_POLL: Duration = Duration::from_secs(1);
const WAIT_NOTICE_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to take a run slot in {}", pool.display())]
#[diagnostic(code(ERR_PNPM_MACHINE_RUN_SLOT_FAILED))]
pub(crate) struct MachineRunSlotError {
    pool: PathBuf,
    #[error(source)]
    source: io::Error,
}

/// One held slot. Dropping it, or the process ending, frees the slot.
pub(crate) struct MachineRunSlot {
    _file: File,
}

/// Take a slot of the configured pool, waiting for one to free up when
/// they are all held. `None` when no limit is configured, or when the
/// invocation runs under a slot a parent already holds.
///
/// `emit` receives a notice when the wait starts and every
/// [`WAIT_NOTICE_EVERY`] after, naming who holds the slots.
pub(crate) fn acquire_machine_run_slot(
    config: &Config,
    emit: fn(&LogEvent),
) -> Result<Option<MachineRunSlot>, MachineRunSlotError> {
    let Some(limit) = config.machine_run_concurrency.filter(|limit| *limit > 0) else {
        return Ok(None);
    };
    let group = &config.machine_run_concurrency_group;
    if std::env::var(MACHINE_RUN_SLOT_ENV).is_ok_and(|held| &held == group) {
        return Ok(None);
    }
    let pool = SlotPool { dir: config.state_dir.join("run-slots").join(group), limit };
    pool.acquire(|| {
        let holders = pool.holders();
        emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: format!(
                r#"Waiting for a free run slot: all {limit} slots of group "{group}" are held ({}). Holders: {}"#,
                pool.dir.display(),
                if holders.is_empty() { "unknown".to_string() } else { holders.join("; ") },
            ),
        }));
    })
    .map(Some)
    .map_err(|source| MachineRunSlotError { pool: pool.dir.clone(), source })
}

/// The slot the parent process holds, for the environment of the scripts
/// and commands a holder spawns.
pub(crate) fn stamp_held_slot(config: &mut Config, slot: Option<&MachineRunSlot>) {
    if slot.is_some() {
        config.extra_env.insert(
            MACHINE_RUN_SLOT_ENV.to_string(),
            config.machine_run_concurrency_group.clone(),
        );
    }
}

struct SlotPool {
    dir: PathBuf,
    limit: u32,
}

impl SlotPool {
    fn acquire(&self, mut on_wait: impl FnMut()) -> io::Result<MachineRunSlot> {
        fs::create_dir_all(&self.dir)?;
        let mut poll = FIRST_POLL;
        let mut last_notice: Option<Instant> = None;
        loop {
            if let Some(slot) = self.try_acquire()? {
                return Ok(slot);
            }
            if last_notice.is_none_or(|at| at.elapsed() >= WAIT_NOTICE_EVERY) {
                on_wait();
                last_notice = Some(Instant::now());
            }
            std::thread::sleep(poll);
            poll = (poll * 2).min(MAX_POLL);
        }
    }

    fn try_acquire(&self) -> io::Result<Option<MachineRunSlot>> {
        for index in 0..self.limit {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.slot_path(index))?;
            match file.try_lock() {
                Ok(()) => {
                    // Best effort: the stamp only feeds the waiting notice.
                    let _ = write_holder(&mut file);
                    return Ok(Some(MachineRunSlot { _file: file }));
                }
                Err(std::fs::TryLockError::WouldBlock) => continue,
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        Ok(None)
    }

    fn slot_path(&self, index: u32) -> PathBuf {
        self.dir.join(index.to_string())
    }

    /// What the holders stamped into the slot files, one entry per slot
    /// that could be read. A slot is read without taking its lock, which
    /// works on the platforms where the lock is advisory and yields nothing
    /// where it is not.
    fn holders(&self) -> Vec<String> {
        (0..self.limit)
            .filter_map(|index| {
                let holder = fs::read_to_string(self.slot_path(index)).ok()?;
                let holder = holder.trim();
                (!holder.is_empty()).then(|| holder.to_string())
            })
            .collect()
    }
}

fn write_holder(file: &mut File) -> io::Result<()> {
    file.set_len(0)?;
    file.rewind()?;
    let cwd = std::env::current_dir().unwrap_or_default();
    write!(file, "pid {} in {}", std::process::id(), cwd.display())?;
    file.flush()
}

#[cfg(test)]
mod tests;
