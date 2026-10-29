//! The one way out of pnpm. What a command leaves running outlives pnpm
//! exactly when it would outlive the command run without pnpm, so every
//! exit pnpm makes on its own goes through here first (see
//! `PROCESS_LIFETIME.md`). `clippy.toml` bans `std::process::exit` everywhere
//! else.

/// Settle what pnpm owes the processes it started before it exits on its
/// own: let the tasks a bail cancelled finish their grace, then release the
/// process-tree cleanup so nothing a command left running ends with pnpm.
/// [`exit`] calls it, and `main` does before returning.
pub fn before_exit() {
    crate::process_tracker::finish_cancellations();
    crate::job_control::release_process_tree_cleanup();
}

/// End pnpm with `code`, after [`before_exit`].
#[expect(clippy::exit, reason = "the one place pnpm exits from")]
#[expect(clippy::disallowed_methods, reason = "the one place pnpm exits from")]
pub fn exit(code: i32) -> ! {
    before_exit();
    std::process::exit(code)
}
