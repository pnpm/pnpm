//! A process table laid out as Linux lays out `/proc`: a directory per
//! process, named by its id, whose `stat` file holds the process's state
//! and process group among its fields.

use std::{fs, io, path::Path, process};

/// Whether the group led by `leader` may still hold a process that has not
/// exited. Only two readings in a row of `table` that show the same members
/// of the group, all of them zombies, rule that out, since a member may
/// start another process and exit while the table is read, and a reading
/// whose listing came first misses the new one. A table that cannot be
/// listed to the end, is not the table of pnpm's own pid namespace, or
/// shows no member at all, as when `hidepid=invisible` hides them, cannot
/// rule it out.
pub(super) fn has_running_member(table: &Path, leader: i32) -> bool {
    is_unsettled(|| only_zombies(table, leader))
}

/// Whether two readings in a row from `read` fail to show the same zombies.
pub(super) fn is_unsettled(mut read: impl FnMut() -> Option<Vec<u32>>) -> bool {
    read().is_none_or(|zombies| read() != Some(zombies))
}

/// The members of the group led by `leader` that `table` shows, if it shows
/// some and all of them are zombies.
fn only_zombies(table: &Path, leader: i32) -> Option<Vec<u32>> {
    let mut pids = process_ids(table).ok()?;
    if !is_own_namespace(table) {
        return None;
    }
    // The script and whatever it started hold the newest ids, so a member
    // that is still running turns up within the first few reads.
    pids.sort_unstable();
    let mut zombies = Vec::new();
    for pid in pids.into_iter().rev() {
        match member_is_running(table, pid, leader) {
            Some(true) => return None,
            Some(false) => zombies.push(pid),
            None => {}
        }
    }
    (!zombies.is_empty()).then_some(zombies)
}

fn process_ids(table: &Path) -> io::Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in fs::read_dir(table)? {
        let name = entry?.file_name();
        if let Some(Ok(pid)) = name.to_str().map(str::parse) {
            pids.push(pid);
        }
    }
    Ok(pids)
}

/// Whether `table` is the process table of pnpm's own pid namespace, whose
/// `self` is pnpm's id. An empty directory in place of `/proc` has no
/// `self`, and the `/proc` of another namespace numbers the processes and
/// their groups differently.
fn is_own_namespace(table: &Path) -> bool {
    fs::read_link(table.join("self")).is_ok_and(|id| id == Path::new(&process::id().to_string()))
}

/// Whether the process `pid` is running, if it belongs to the group led by
/// `leader`. A process whose `stat` cannot be read, as another user's under
/// `hidepid=1`, counts as running while the kernel still has it in the
/// group, since nothing shows that it has exited.
fn member_is_running(table: &Path, pid: u32, leader: i32) -> Option<bool> {
    let entry = table.join(pid.to_string());
    let Ok(stat) = fs::read(entry.join("stat")) else {
        return is_in_group(pid, leader).then_some(true);
    };
    let (state, group) = state_and_group(&stat)?;
    (group == leader).then(|| state != b'Z' || has_other_threads(&entry))
}

/// Whether the process at `entry` has a thread besides its main one. A
/// process whose main thread exited while others keep running reads as a
/// zombie in its `stat`, but lists those threads under `task`.
fn has_other_threads(entry: &Path) -> bool {
    fs::read_dir(entry.join("task")).is_ok_and(|threads| threads.count() > 1)
}

fn is_in_group(pid: u32, leader: i32) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    // SAFETY: `getpgid` only reads the process group of `pid`, and returns
    // -1, never a group, once no such process exists.
    unsafe { libc::getpgid(pid) == leader }
}

/// The state and the process group in a `stat` line. Both follow the
/// command, which is in parentheses and may hold any byte, `)` included, so
/// the fields are counted from the last `)`.
fn state_and_group(stat: &[u8]) -> Option<(u8, i32)> {
    let command_end = stat
        .iter()
        .rposition(|&byte| byte == b')')?;
    let mut fields = stat[command_end + 1..]
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty());
    let state = *fields.next()?.first()?;
    let group = std::str::from_utf8(fields.nth(1)?)
        .ok()?
        .parse()
        .ok()?;
    Some((state, group))
}
