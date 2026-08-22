use crate::{LoadLockfileError, Lockfile, WantedLockfileSelection};
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
    source: Option<(PathBuf, WantedLockfileSelection)>,
    cell: OnceLock<Option<Lockfile>>,
}

impl LazyLockfile {
    /// A lockfile that will be loaded from `dir` (the same source as
    /// [`Lockfile::load_wanted`]) on first [`Self::get`].
    #[must_use]
    pub fn deferred(dir: PathBuf, selection: WantedLockfileSelection) -> Self {
        LazyLockfile { source: Some((dir, selection)), cell: OnceLock::new() }
    }

    /// A lockfile that is never loaded — [`Self::get`] yields `None`
    /// without touching the filesystem. Mirrors `lockfile: false`
    /// config.
    #[must_use]
    pub fn disabled() -> Self {
        LazyLockfile { source: None, cell: OnceLock::new() }
    }

    /// A lockfile that is already in memory; [`Self::get`] returns it
    /// without touching the filesystem.
    #[must_use]
    pub fn preloaded(lockfile: Option<Lockfile>) -> Self {
        let cell = OnceLock::new();
        cell.set(lockfile).expect("a fresh OnceLock accepts the first set");
        LazyLockfile { source: None, cell }
    }

    /// The parsed wanted lockfile, loading it on first call. `None`
    /// when the file is absent, empty, or loading is disabled. A load
    /// error is returned without being cached, so a subsequent call
    /// retries — callers abort on the first error in practice.
    pub fn get(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        if let Some(lockfile) = self.cell.get() {
            return Ok(lockfile.as_ref());
        }
        let loaded = match self.source.as_ref() {
            Some((dir, selection)) => Lockfile::load_wanted(dir, selection)?,
            None => None,
        };
        Ok(self.cell.get_or_init(|| loaded).as_ref())
    }

    /// Whether a wanted lockfile is known to be available: the parsed
    /// document when already loaded, otherwise
    /// [`Lockfile::wanted_exists`]'s semantic-presence probe —
    /// the same absence rules as the loader (an empty or env-only
    /// file counts as absent), without paying for the YAML parse on
    /// the repeat-install fast path.
    #[must_use]
    pub fn is_loaded_or_on_disk(&self) -> bool {
        if let Some(lockfile) = self.cell.get() {
            return lockfile.is_some();
        }
        self.source
            .as_ref()
            .is_some_and(|(dir, selection)| Lockfile::wanted_exists(dir, &selection.file_name))
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
