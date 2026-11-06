//! The calling process's user identity.

/// The effective user id, the one the kernel checks a file's owner against.
#[must_use]
pub fn effective_user_id() -> u32 {
    // SAFETY: `geteuid` reads a kernel-owned field of the calling process.
    // It takes no arguments, writes nothing, and cannot fail, so there is no
    // precondition for this call to establish.
    unsafe { libc::geteuid() }
}

/// The real user id, the account that started the process.
#[cfg(not(target_os = "cygwin"))]
#[must_use]
pub fn real_user_id() -> u32 {
    // SAFETY: as in `effective_user_id`, `getuid` reads a kernel-owned field
    // of the calling process and has no precondition.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests;
