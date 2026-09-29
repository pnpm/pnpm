//! A process table laid out as Linux lays out `/proc`: a directory per
//! process, named by its id, whose `stat` file holds the process's state
//! and process group among its fields.

use std::{fs, io, path::Path, process};

/// Whether `table` lists a process of the group led by `leader` that has
/// not exited, or cannot rule one out because it cannot be listed to the
/// end or is not the table of pnpm's own pid namespace.
///
/// A process whose `stat` cannot be read, as another user's under
/// `hidepid=1`, counts as running while the kernel still has it in the
/// group, since nothing shows that it has exited.
pub(super) fn has_running_member(table: &Path, leader: i32) -> bool {
    let Ok(mut pids) = process_ids(table) else { return true };
    if !is_own_namespace(table) {
        return true;
    }
    // The script and whatever it started hold the newest ids, so a member
    // that is still running turns up within the first few reads.
    pids.sort_unstable();
    pids.into_iter()
        .rev()
        .any(|pid| is_running_member(table, pid, leader))
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

fn is_running_member(table: &Path, pid: u32, leader: i32) -> bool {
    let Ok(stat) = fs::read(table.join(pid.to_string()).join("stat")) else {
        return is_in_group(pid, leader);
    };
    state_and_group(&stat).is_some_and(|(state, group)| state != b'Z' && group == leader)
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
