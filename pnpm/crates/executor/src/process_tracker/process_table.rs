//! A process table laid out as Linux lays out `/proc`: a directory per
//! process, named by its id, whose `stat` file holds the process's state
//! and process group among its fields.

use std::{fs, io, path::Path};

/// Whether `table` lists a process of the group led by `leader` that has
/// not exited, or cannot be listed to the end and so cannot rule one out.
///
/// An entry whose `stat` cannot be read is not counted. Its process exited
/// after the listing, or it belongs to another user under a restricted
/// table, and either way it is not a member pnpm could wait for.
pub(super) fn has_running_member(table: &Path, leader: i32) -> bool {
    let Ok(mut pids) = process_ids(table) else { return true };
    // The script and whatever it started hold the newest ids, so a member
    // that is still running turns up within the first few reads.
    pids.sort_unstable();
    pids.into_iter()
        .rev()
        .any(|pid| is_running_member(&table.join(pid.to_string()), leader))
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

fn is_running_member(entry: &Path, leader: i32) -> bool {
    let Ok(stat) = fs::read(entry.join("stat")) else { return false };
    state_and_group(&stat).is_some_and(|(state, group)| state != b'Z' && group == leader)
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
