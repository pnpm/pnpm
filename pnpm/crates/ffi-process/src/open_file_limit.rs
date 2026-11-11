//! The calling process's limit on open descriptors, `RLIMIT_NOFILE`.

/// The soft limit on open descriptors, the one a further `open` is checked
/// against. `None` when the limit cannot be read or does not fit a `usize`.
#[must_use]
pub fn soft_open_file_limit() -> Option<usize> {
    usize::try_from(current_limit()?.rlim_cur).ok()
}

/// Raise the soft limit on open descriptors as far as the hard limit and
/// `ceiling` allow.
///
/// Lowering is not offered: raising the soft limit up to the hard one needs
/// no privileges, and nothing in pnpm has a reason to give capacity back. A
/// limit that cannot be raised is left alone rather than reported as an
/// error, since every caller's fallback is to run under the limit it has.
pub fn raise_open_file_limit(ceiling: u64) {
    let Some(limit) = current_limit() else { return };
    let target = limit.rlim_max.min(ceiling);
    if target > limit.rlim_cur && set_soft_limit(target, limit.rlim_max) {
        return;
    }
    // macOS rejects a soft limit above `kern.maxfilesperproc` even when the
    // hard limit reads unlimited. 10240 is the historically safe `OPEN_MAX`
    // there, and no other platform has a second ceiling worth retrying
    // under.
    if cfg!(target_os = "macos") {
        let fallback = limit.rlim_max.min(10_240);
        if fallback > limit.rlim_cur {
            set_soft_limit(fallback, limit.rlim_max);
        }
    }
}

fn current_limit() -> Option<libc::rlimit> {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: the pointer is to a stack local that outlives the call, of
    // exactly the type `getrlimit` writes, and both of whose fields are
    // initialized beforehand so a call that fails without writing still
    // leaves the local readable.
    let read = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) };
    (read == 0).then_some(limit)
}

fn set_soft_limit(soft: libc::rlim_t, hard: libc::rlim_t) -> bool {
    let request = libc::rlimit { rlim_cur: soft, rlim_max: hard };
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives the call, of exactly the type `setrlimit` reads. The call
    // retains no pointer.
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const request) == 0 }
}

#[cfg(test)]
mod tests;
