use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use std::path::Path;

/// The wanted lockfile as a manifest-mutating command received it.
///
/// `add`, `remove` and `update` read the lockfile themselves — for
/// preferred versions, catalog requests, drop targets — and then run an
/// install over it, so all three views travel together.
#[derive(Clone, Copy)]
pub struct CommandLockfile<'a> {
    /// The document the command reads, resolved out of [`Self::source`]
    /// once by the caller so those reads stay infallible.
    pub document: Option<&'a Lockfile>,
    /// The loader [`Self::document`] came from, handed to the install so
    /// it can report and gate on a Git-conflict merge that load
    /// performed. [`MaybeLazyLockfile::Loaded`] stands for a lockfile
    /// that was never read from disk, which no recovery can have
    /// touched.
    pub source: MaybeLazyLockfile<'a>,
    /// Absolute path of the loaded `pnpm-lock.yaml`, threaded into the
    /// lockfile-verification gate and its reporter payload. `None`
    /// disables the per-path cache for this run and falls back to
    /// deriving the path from the workspace root.
    pub path: Option<&'a Path>,
}

impl<'a> CommandLockfile<'a> {
    /// A lockfile that was never read from disk — synthesized, or handed
    /// over by a caller that already holds it.
    #[must_use]
    pub fn loaded(document: Option<&'a Lockfile>, path: Option<&'a Path>) -> Self {
        CommandLockfile { document, source: MaybeLazyLockfile::Loaded(document), path }
    }
}
