//! The calling process's file-creation mask.

/// The process's file-creation mask.
///
/// POSIX offers no way to read the mask without writing it: `umask(2)`
/// returns the previous value, so a reader has to set one and put the old
/// one back. The window between the two calls is process-wide, and a file
/// another thread creates inside it is created unmasked. Linux publishes
/// the mask in `/proc/self/status`, which is read here first so that the
/// window never opens there.
#[must_use]
pub fn current_umask() -> u32 {
    #[cfg(target_os = "linux")]
    if let Some(mask) = published_umask() {
        return mask;
    }
    read_umask_by_writing_it()
}

/// The mask as Linux publishes it, in the `Umask:` line of
/// `/proc/self/status`. `None` where `procfs` is not mounted, or on a
/// kernel before 4.7, which does not print the line.
#[cfg(target_os = "linux")]
fn published_umask() -> Option<u32> {
    parse_published_umask(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Separate from [`published_umask`] so a test can drive it without a
/// `procfs` whose contents it cannot choose.
#[cfg(target_os = "linux")]
fn parse_published_umask(status: &str) -> Option<u32> {
    let field = status
        .lines()
        .find_map(|line| line.strip_prefix("Umask:"))?;
    u32::from_str_radix(field.trim(), 8).ok()
}

/// Set the mask to `0` to learn what it was, then put it back.
///
/// The fallback for a target that does not publish the mask, and the whole
/// of the implementation on one that has no `procfs`. Both calls are made
/// with nothing between them, so the unmasked window is as short as two
/// syscalls.
fn read_umask_by_writing_it() -> u32 {
    // SAFETY: `umask` takes its argument by value, writes no memory, and
    // cannot fail. The pair leaves the process's mask as it found it: the
    // first call reports the mask it replaced, and the second restores
    // exactly that value.
    let mask = unsafe {
        let previous = libc::umask(0);
        libc::umask(previous);
        previous
    };
    widen_mode(mask)
}

/// `libc::umask` speaks `mode_t`, a `u16` on macOS and a `u32` on Linux.
/// `Into` is the one widening both platforms' clippy accepts: a cast is
/// `cast_lossless` on macOS and `u32::from` is `useless_conversion` on
/// Linux.
fn widen_mode(mode: impl Into<u32>) -> u32 {
    mode.into()
}

#[cfg(test)]
mod tests;
