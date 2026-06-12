use crate::{LoadLockfileError, Lockfile};
use std::{path::Path, sync::OnceLock};

/// Wanted lockfile (`pnpm-lock.yaml`) whose read + parse are deferred
/// until a consumer actually needs the contents.
///
/// The optimistic repeat-install fast path decides "Already up to
/// date" from manifest mtimes alone — upstream's `checkDepsStatus`
/// never reads the wanted lockfile on that path — so parsing a
/// multi-megabyte YAML document up front is pure overhead for the
/// repeat-install case. Commands that always need the lockfile call
/// [`LazyLockfile::get`] immediately and behave as if it were loaded
/// eagerly.
pub struct LazyLockfile {
    /// `false` mirrors `lockfile: false` config: [`Self::get`] yields
    /// `None` without touching the filesystem, matching the eager
    /// loader's "don't even read the file" behavior.
    enabled: bool,
    cell: OnceLock<Option<Lockfile>>,
}

impl LazyLockfile {
    /// A lockfile that will be loaded from the current directory (the
    /// same source as [`Lockfile::load_from_current_dir`]) on first
    /// [`Self::get`]. `enabled: false` skips the load entirely.
    pub fn deferred(enabled: bool) -> Self {
        LazyLockfile { enabled, cell: OnceLock::new() }
    }

    /// A lockfile that is already in memory; [`Self::get`] returns it
    /// without touching the filesystem.
    pub fn preloaded(lockfile: Option<Lockfile>) -> Self {
        let cell = OnceLock::new();
        cell.set(lockfile).expect("a fresh OnceLock accepts the first set");
        LazyLockfile { enabled: true, cell }
    }

    /// The parsed wanted lockfile, loading it on first call. `None`
    /// when the file is absent, empty, or loading is disabled. A load
    /// error is returned without being cached, so a subsequent call
    /// retries — callers abort on the first error in practice.
    pub fn get(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        if let Some(lockfile) = self.cell.get() {
            return Ok(lockfile.as_ref());
        }
        let loaded = if self.enabled { Lockfile::load_from_current_dir()? } else { None };
        Ok(self.cell.get_or_init(|| loaded).as_ref())
    }

    /// Whether a wanted lockfile is known to be available: the parsed
    /// document when already loaded, otherwise a filesystem existence
    /// probe — deliberately not a parse, so the repeat-install fast
    /// path stays mtime-cheap.
    pub fn is_loaded_or_on_disk(&self) -> bool {
        if let Some(lockfile) = self.cell.get() {
            return lockfile.is_some();
        }
        self.enabled && Path::new(Lockfile::FILE_NAME).exists()
    }
}

/// A wanted lockfile that is either already parsed (callers that
/// re-resolve after a manifest mutation hold one) or lazily loadable.
/// `Copy` so it threads through the install pipeline like the
/// `Option<&Lockfile>` it replaces.
#[derive(Clone, Copy)]
pub enum MaybeLazyLockfile<'a> {
    Loaded(Option<&'a Lockfile>),
    Lazy(&'a LazyLockfile),
}

impl<'a> MaybeLazyLockfile<'a> {
    /// The parsed wanted lockfile, loading it now when lazy. See
    /// [`LazyLockfile::get`] for the error contract.
    pub fn get(self) -> Result<Option<&'a Lockfile>, LoadLockfileError> {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => Ok(lockfile),
            MaybeLazyLockfile::Lazy(lazy) => lazy.get(),
        }
    }

    /// Whether a wanted lockfile is available, without forcing a parse
    /// in the lazy case. See [`LazyLockfile::is_loaded_or_on_disk`].
    pub fn is_loaded_or_on_disk(self) -> bool {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => lockfile.is_some(),
            MaybeLazyLockfile::Lazy(lazy) => lazy.is_loaded_or_on_disk(),
        }
    }
}

#[cfg(test)]
mod tests;
