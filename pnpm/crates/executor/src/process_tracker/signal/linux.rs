//! Reading a process's parent from `/proc`.
//!
//! Pinning, signalling and waiting on a pinned process live in
//! `pnpm-ffi-signal`, which owns those calls.

/// The parent pid `/proc` gives `pid`.
pub(super) fn parent_of(pid: i32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
