use crate::{LoadLockfileError, Lockfile};
use std::{path::PathBuf, sync::OnceLock};

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
    dir: Option<PathBuf>,
    cell: OnceLock<Option<Lockfile>>,
}

impl LazyLockfile {
    /// A lockfile that will be loaded from `<dir>/pnpm-lock.yaml` (the
    /// same source as [`Lockfile::load_wanted_from_dir`]) on first
    /// [`Self::get`].
    #[must_use]
    pub fn deferred(dir: PathBuf) -> Self {
        LazyLockfile { dir: Some(dir), cell: OnceLock::new() }
    }

    /// A lockfile that is never loaded — [`Self::get`] yields `None`
    /// without touching the filesystem. Mirrors `lockfile: false`
    /// config.
    #[must_use]
    pub fn disabled() -> Self {
        LazyLockfile { dir: None, cell: OnceLock::new() }
    }

    /// A lockfile that is already in memory; [`Self::get`] returns it
    /// without touching the filesystem.
    #[must_use]
    pub fn preloaded(lockfile: Option<Lockfile>) -> Self {
        let cell = OnceLock::new();
        cell.set(lockfile).expect("a fresh OnceLock accepts the first set");
        LazyLockfile { dir: None, cell }
    }

    /// The parsed wanted lockfile, loading it on first call. `None`
    /// when the file is absent, empty, or loading is disabled. A load
    /// error is returned without being cached, so a subsequent call
    /// retries — callers abort on the first error in practice.
    pub fn get(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        if let Some(lockfile) = self.cell.get() {
            return Ok(lockfile.as_ref());
        }
        let loaded = match self.dir.as_deref() {
            Some(dir) => Lockfile::load_wanted_from_dir(dir)?,
            None => None,
        };
        Ok(self.cell.get_or_init(|| loaded).as_ref())
    }

    /// Whether a wanted lockfile is known to be available: the parsed
    /// document when already loaded, otherwise
    /// [`Lockfile::wanted_exists_in_dir`]'s semantic-presence probe —
    /// the same absence rules as the loader (an empty or env-only
    /// file counts as absent), without paying for the YAML parse on
    /// the repeat-install fast path.
    #[must_use]
    pub fn is_loaded_or_on_disk(&self) -> bool {
        if let Some(lockfile) = self.cell.get() {
            return lockfile.is_some();
        }
        self.dir.as_deref().is_some_and(Lockfile::wanted_exists_in_dir)
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
    #[must_use]
    pub fn is_loaded_or_on_disk(self) -> bool {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => lockfile.is_some(),
            MaybeLazyLockfile::Lazy(lazy) => lazy.is_loaded_or_on_disk(),
        }
    }
}

#[cfg(test)]
mod tests;
