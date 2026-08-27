use crate::{
    LoadLockfileError, LoadedWantedLockfile, Lockfile, ProjectSnapshot, WantedLockfileSelection,
};
use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

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
    cell: OnceLock<LoadedWantedLockfile>,
    fix_cell: OnceLock<LoadedWantedLockfile>,
    fix_merge_cell: OnceLock<LoadedWantedLockfile>,
}

impl LazyLockfile {
    /// A lockfile that will be loaded from `dir` (the same source as
    /// [`Lockfile::load_wanted`]) on first [`Self::get`].
    #[must_use]
    pub fn deferred(dir: PathBuf, selection: WantedLockfileSelection) -> Self {
        LazyLockfile {
            source: Some((dir, selection)),
            cell: OnceLock::new(),
            fix_cell: OnceLock::new(),
            fix_merge_cell: OnceLock::new(),
        }
    }

    /// A lockfile that is never loaded — [`Self::get`] yields `None`
    /// without touching the filesystem. Mirrors `lockfile: false`
    /// config.
    #[must_use]
    pub fn disabled() -> Self {
        LazyLockfile {
            source: None,
            cell: OnceLock::new(),
            fix_cell: OnceLock::new(),
            fix_merge_cell: OnceLock::new(),
        }
    }

    /// A lockfile that is already in memory; [`Self::get`] returns it
    /// without touching the filesystem.
    #[must_use]
    pub fn preloaded(lockfile: Option<Lockfile>) -> Self {
        let cell = OnceLock::new();
        cell.set(LoadedWantedLockfile { lockfile, pre_merge_importers: None })
            .expect("a fresh OnceLock accepts the first set");
        LazyLockfile {
            source: None,
            cell,
            fix_cell: OnceLock::new(),
            fix_merge_cell: OnceLock::new(),
        }
    }

    /// The parsed wanted lockfile, loading it on first call. `None`
    /// when the file is absent, empty, or loading is disabled. A load
    /// error is returned without being cached, so a subsequent call
    /// retries — callers abort on the first error in practice.
    pub fn get(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        Ok(self.load()?.lockfile.as_ref())
    }

    /// Load after discarding fields that a repairing resolution regenerates.
    pub fn get_for_fix(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        Ok(self.load_for_fix()?.lockfile.as_ref())
    }

    fn get_for_fix_merge(&self) -> Result<Option<&Lockfile>, LoadLockfileError> {
        Ok(self.load_for_fix_merge()?.lockfile.as_ref())
    }

    /// The importers the branch-lockfile fold started from, loading the
    /// lockfile on first call. `None` when no fold was attempted. See
    /// [`LoadedWantedLockfile`] for why the caller needs them.
    pub fn pre_merge_importers(
        &self,
    ) -> Result<Option<&HashMap<String, ProjectSnapshot>>, LoadLockfileError> {
        Ok(self.load()?.pre_merge_importers.as_ref())
    }

    fn pre_merge_importers_for_fix(
        &self,
    ) -> Result<Option<&HashMap<String, ProjectSnapshot>>, LoadLockfileError> {
        Ok(self.load_for_fix()?.pre_merge_importers.as_ref())
    }

    fn load(&self) -> Result<&LoadedWantedLockfile, LoadLockfileError> {
        if let Some(loaded) = self.cell.get() {
            return Ok(loaded);
        }
        let loaded = match self.source.as_ref() {
            Some((dir, selection)) => Lockfile::load_wanted_detailed(dir, selection)?,
            None => LoadedWantedLockfile::default(),
        };
        Ok(self.cell.get_or_init(|| loaded))
    }

    fn load_for_fix(&self) -> Result<&LoadedWantedLockfile, LoadLockfileError> {
        if let Some(loaded) = self.fix_cell.get() {
            return Ok(loaded);
        }
        let loaded = if let Some((dir, selection)) = self.source.as_ref() {
            Lockfile::load_wanted_detailed_for_fix(dir, selection)?
        } else {
            let mut loaded = self.cell.get().cloned().unwrap_or_default();
            if let Some(lockfile) = loaded.lockfile.as_mut() {
                lockfile.prepare_for_fix();
            }
            loaded
        };
        Ok(self.fix_cell.get_or_init(|| loaded))
    }

    fn load_for_fix_merge(&self) -> Result<&LoadedWantedLockfile, LoadLockfileError> {
        if let Some(loaded) = self.fix_merge_cell.get() {
            return Ok(loaded);
        }
        let loaded = if let Some((dir, selection)) = self.source.as_ref() {
            Lockfile::load_wanted_detailed_for_fix_merge(dir, selection)?
        } else {
            self.cell.get().cloned().unwrap_or_default()
        };
        Ok(self.fix_merge_cell.get_or_init(|| loaded))
    }

    /// Whether a wanted lockfile is known to be available: the parsed
    /// document when already loaded, otherwise
    /// [`Lockfile::wanted_exists`]'s semantic-presence probe —
    /// the same absence rules as the loader (an empty or env-only
    /// file counts as absent), without paying for the YAML parse on
    /// the repeat-install fast path.
    #[must_use]
    pub fn is_loaded_or_on_disk(&self) -> bool {
        if let Some(loaded) =
            self.cell.get().or_else(|| self.fix_cell.get()).or_else(|| self.fix_merge_cell.get())
        {
            return loaded.lockfile.is_some();
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
    Repair(&'a LazyLockfile),
}

impl<'a> MaybeLazyLockfile<'a> {
    /// The parsed wanted lockfile, loading it now when lazy. See
    /// [`LazyLockfile::get`] for the error contract.
    pub fn get(self) -> Result<Option<&'a Lockfile>, LoadLockfileError> {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => Ok(lockfile),
            MaybeLazyLockfile::Lazy(lazy) => lazy.get(),
            MaybeLazyLockfile::Repair(lazy) => lazy.get_for_fix(),
        }
    }

    /// The intact lockfile used to restore projects outside a filtered repair.
    pub fn get_for_merge(self) -> Result<Option<&'a Lockfile>, LoadLockfileError> {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => Ok(lockfile),
            MaybeLazyLockfile::Lazy(lazy) => lazy.get(),
            MaybeLazyLockfile::Repair(lazy) => lazy.get().or_else(|_| lazy.get_for_fix_merge()),
        }
    }

    /// Whether a wanted lockfile is available, without forcing a parse
    /// in the lazy case. See [`LazyLockfile::is_loaded_or_on_disk`].
    #[must_use]
    pub fn is_loaded_or_on_disk(self) -> bool {
        match self {
            MaybeLazyLockfile::Loaded(lockfile) => lockfile.is_some(),
            MaybeLazyLockfile::Lazy(lazy) | MaybeLazyLockfile::Repair(lazy) => {
                lazy.is_loaded_or_on_disk()
            }
        }
    }

    /// See [`LazyLockfile::pre_merge_importers`]. An already-parsed
    /// lockfile reached the caller through a path that does no folding,
    /// so it never has them.
    pub fn pre_merge_importers(
        self,
    ) -> Result<Option<&'a HashMap<String, ProjectSnapshot>>, LoadLockfileError> {
        match self {
            MaybeLazyLockfile::Loaded(_) => Ok(None),
            MaybeLazyLockfile::Lazy(lazy) => lazy.pre_merge_importers(),
            MaybeLazyLockfile::Repair(lazy) => lazy.pre_merge_importers_for_fix(),
        }
    }
}

#[cfg(test)]
mod tests;
