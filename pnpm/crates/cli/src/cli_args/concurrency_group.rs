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
//! Waiters line up on ticket files beside the slots. A waiter whose process
//! has ended is skipped because that lock is gone, the same way a killed
//! holder frees its slot.
//!
//! A holder stamps the group into [`HELD_CONCURRENCY_GROUPS_ENV`] for the
//! scripts it spawns. A nested `pnpm run` that finds a task's group there
//! runs under the slot its parent holds, which is what keeps a script that
//! calls `pnpm run` from waiting on itself.

pub(crate) mod pool;
pub(crate) mod stamp;
pub(crate) use pool::GroupStatus;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use pool::{SlotPool, WaitSnapshot};
use stamp::format_elapsed;
use std::{
    collections::HashMap,
    fmt::Write,
    fs::File,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// Env var that carries the groups the parent invocations hold slots of,
/// comma-separated.
pub(crate) const HELD_CONCURRENCY_GROUPS_ENV: &str = "PNPM_HELD_CONCURRENCY_GROUPS";

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

/// What taking a slot for a task came to.
pub(crate) enum SlotOutcome {
    /// The task names no limited group, or a parent invocation already
    /// holds a slot of its group.
    Ungated,
    Held(ConcurrencyGroupSlot),
    /// The run was cancelled while the task was waiting for a slot.
    Cancelled,
}

/// Take a slot of the group the task named `script` belongs to, waiting
/// for one to free up when they are all held. `cancelled` is consulted
/// between attempts, so a run that bails does not wait out a holder in
/// another process.
///
/// `emit` receives a notice when the wait starts and every 30s after,
/// naming who holds the slots and this run's place in line.
pub(crate) fn acquire_concurrency_group_slot(
    config: &Config,
    script: &str,
    emit: fn(&LogEvent),
    cancelled: &dyn Fn() -> bool,
) -> Result<SlotOutcome, ConcurrencyGroupSlotError> {
    let inherited = std::env::var(HELD_CONCURRENCY_GROUPS_ENV).ok();
    acquire_slot(config, script, emit, cancelled, inherited.as_deref())
}

/// [`acquire_concurrency_group_slot`] with the inherited held groups
/// passed in, so a test needs no process environment.
fn acquire_slot(
    config: &Config,
    script: &str,
    emit: fn(&LogEvent),
    cancelled: &dyn Fn() -> bool,
    inherited: Option<&str>,
) -> Result<SlotOutcome, ConcurrencyGroupSlotError> {
    let Some((group, limit)) = limited_group(config, script) else {
        return Ok(SlotOutcome::Ungated);
    };
    if held_groups(inherited).any(|held| held == group) {
        return Ok(SlotOutcome::Ungated);
    }
    let priority = task_priority(config, script);
    let pool = SlotPool { dir: config.state_dir.join("run-slots").join(group), limit };
    let on_wait = |snapshot: &WaitSnapshot| {
        emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: format_wait_notice(script, group, limit, &pool.dir, snapshot),
        }));
    };
    pool.acquire(priority, on_wait, cancelled)
        .map(|file| match file {
            Some(file) => {
                SlotOutcome::Held(ConcurrencyGroupSlot { group: group.to_string(), _file: file })
            }
            None => SlotOutcome::Cancelled,
        })
        .map_err(|source| ConcurrencyGroupSlotError {
            group: group.to_string(),
            pool: pool.dir.clone(),
            source,
        })
}

fn format_wait_notice(
    script: &str,
    group: &str,
    limit: u32,
    pool: &Path,
    snapshot: &WaitSnapshot,
) -> String {
    let holders = if snapshot.holders.is_empty() {
        "unknown".to_string()
    } else {
        snapshot.holders.join("; ")
    };
    let mut message = format!(
        r#"Waiting to run "{script}": all {limit} slots of concurrency group "{group}" are held ({}). You are #{} of {} in line. Holders: {holders}"#,
        pool.display(),
        snapshot.position,
        snapshot.total,
    );
    if !snapshot.ahead.is_empty() {
        message.push_str(". Ahead: ");
        message.push_str(&snapshot.ahead.join("; "));
    }
    message
}

/// The group the task named `script` is in, with its limit, when both
/// are configured and the limit is positive.
fn limited_group<'a>(config: &'a Config, script: &str) -> Option<(&'a str, u32)> {
    let group = config.tasks.get(script)?.concurrency_group.as_deref()?;
    let limit = config.concurrency_groups
        .get(group)
        .copied()
        .filter(|limit| *limit > 0)?;
    Some((group, limit))
}

fn task_priority(config: &Config, script: &str) -> i32 {
    config.tasks
        .get(script)
        .and_then(|task| task.priority)
        .unwrap_or(0)
}

/// `extra_env` with `group` added to the held groups the spawned script
/// sees, on top of the ones this process inherited.
pub(crate) fn with_held_group(
    extra_env: &HashMap<String, String>,
    group: &str,
) -> HashMap<String, String> {
    let inherited = std::env::var(HELD_CONCURRENCY_GROUPS_ENV).ok();
    add_held_group(extra_env, inherited.as_deref(), group)
}

fn add_held_group(
    extra_env: &HashMap<String, String>,
    inherited: Option<&str>,
    group: &str,
) -> HashMap<String, String> {
    let held: Vec<&str> = held_groups(inherited)
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

/// Live holders and waiters of one group directory. A missing directory
/// is idle.
pub(crate) fn inspect_group(dir: &Path) -> io::Result<GroupStatus> {
    if !dir.is_dir() {
        return Ok(GroupStatus { holders: Vec::new(), waiters: Vec::new() });
    }
    SlotPool { dir: dir.to_path_buf(), limit: 0 }.status()
}

pub(crate) fn render_group(name: &str, status: &GroupStatus) -> String {
    if status.is_idle() {
        return format!("{name}: idle");
    }
    let mut out = name.to_string();
    if !status.holders.is_empty() {
        out.push_str("\n  running");
        for holder in &status.holders {
            append_status_line(&mut out, None, &holder.info, holder.elapsed, None);
        }
    }
    if !status.waiters.is_empty() {
        out.push_str("\n  waiting");
        for (index, waiter) in status.waiters.iter().enumerate() {
            append_status_line(
                &mut out,
                Some(index + 1),
                &waiter.info,
                waiter.elapsed,
                (waiter.priority != 0).then_some(waiter.priority),
            );
        }
    }
    out
}

fn append_status_line(
    out: &mut String,
    position: Option<usize>,
    info: &str,
    elapsed: Option<Duration>,
    priority: Option<i32>,
) {
    match position {
        Some(position) => {
            let _ = write!(out, "\n    {position}. {info}");
        }
        None => {
            let _ = write!(out, "\n    {info}");
        }
    }
    if let Some(elapsed) = elapsed {
        let _ = write!(out, "  {}", format_elapsed(elapsed));
    }
    if let Some(priority) = priority {
        let _ = write!(out, "  priority {priority}");
    }
}

#[cfg(test)]
mod tests;
