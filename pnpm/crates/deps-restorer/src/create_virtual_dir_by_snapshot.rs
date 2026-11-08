pub(super) mod slot_paths;
#[cfg(test)]
pub mod tests;
pub use error::CreateVirtualDirError;
mod error;
use slot_paths::{
    OpenedSlot, SlotForceInputs, SlotPaths, SlotReimport, cas_paths_with_build_marker,
    slot_import_opts,
};

use crate::{
    DirCloneCache, NEEDS_BUILD_MARKER, VirtualStoreLayout, create_symlink_layout,
    import_indexed_dir, import_indexed_dir::marker_present,
    safe_join_modules_dir::safe_join_modules_dir,
};
use pnpm_config::PackageImportMethod;
use pnpm_fs::{is_subdir, is_symlink_or_junction, remove_symlink_dir};
use pnpm_lockfile::{PackageKey, PkgName, SnapshotEntry};
use pnpm_reporter::{
    LogEvent, LogLevel, PackageImportMethod as WireImportMethod, ProgressLog, ProgressMessage,
    Reporter,
};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
};

/// This subroutine creates the virtual-store slot for one package and then
/// runs the two post-extraction tasks — CAS file import and intra-package
/// symlink creation — in parallel via `rayon::join`.
///
/// Symlinks don't depend on CAS file contents, only on the resolved dep graph,
/// so overlapping them with the import saves the serial symlink time per
/// snapshot (~1-3 ms). Across a big lockfile those savings stack up on the
/// install's critical-path tail.
#[must_use]
pub struct CreateVirtualDirBySnapshot<'a> {
    pub dependencies: crate::SnapshotDependencyLinks<'a>,
    pub import: crate::PackageImportOptions<'a>,
    pub source: crate::SlotImportSource<'a>,
    /// Per-install precomputed slot-directory mapping. The layout
    /// holds the root and knows how to resolve a per-snapshot slot
    /// (legacy `<root>/<flat-name>` vs GVS-shaped
    /// `<root>/<scope>/<name>/<version>/<hash>`) through a single
    /// [`VirtualStoreLayout::slot_dir`] lookup. See
    /// [`crate::VirtualStoreLayout`] for how it's built.
    pub layout: &'a VirtualStoreLayout,
    pub cas_paths: &'a pnpm_fs::CasPaths,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`.
    /// Currently unused by `imported` (whose payload doesn't carry
    /// `packageId`) but kept here so future progress channels (e.g.
    /// per-package counts) can read it without rethreading.
    pub package_id: &'a str,
    /// macOS directory-clone materialization cache
    /// ([`crate::dir_clone_cache`]). `None` when the install isn't
    /// eligible ([`DirCloneCache::eligible`]) or when the caller's
    /// per-slot qualification says this slot must take the per-file
    /// import.
    pub dir_clone_cache: Option<&'a DirCloneCache<'a>>,
    #[cfg(test)]
    pub link_concurrency_probe: Option<&'a tests::LinkConcurrencyProbe>,
}

/// What [`CreateVirtualDirBySnapshot::try_run`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRun {
    Linked,
    /// Another install holds the slot's lock, and nothing was written.
    Busy,
}

impl CreateVirtualDirBySnapshot<'_> {
    /// Execute the subroutine, waiting for the slot's lock.
    pub fn run<Reporter: self::Reporter>(self) -> Result<(), CreateVirtualDirError> {
        let paths = SlotPaths::new(self.layout, self.dependencies.package_key)?;
        let global_slot =
            crate::GlobalVirtualStoreSlot::acquire(self.layout, self.dependencies.package_key)
                .map_err(CreateVirtualDirError::SlotLock)?;
        self.link_slot::<Reporter>(paths, global_slot)
    }

    /// [`Self::run`] that returns [`SlotRun::Busy`] instead of waiting for
    /// another install's slot lock. Rayon workers link slots this way: a
    /// worker waiting inside the import's `rayon::join` can steal another
    /// slot's job, so a blocking wait there would hold one slot lock while
    /// waiting for another.
    pub fn try_run<Reporter: self::Reporter>(self) -> Result<SlotRun, CreateVirtualDirError> {
        let paths = SlotPaths::new(self.layout, self.dependencies.package_key)?;
        match crate::GlobalVirtualStoreSlot::try_acquire(self.layout, self.dependencies.package_key)
            .map_err(CreateVirtualDirError::SlotLock)?
        {
            crate::SlotAttempt::Busy => Ok(SlotRun::Busy),
            crate::SlotAttempt::Acquired(global_slot) => {
                self.link_slot::<Reporter>(paths, global_slot)?;
                Ok(SlotRun::Linked)
            }
        }
    }

    fn link_slot<Reporter: self::Reporter>(
        self,
        paths: SlotPaths,
        global_slot: Option<crate::GlobalVirtualStoreSlot>,
    ) -> Result<(), CreateVirtualDirError> {
        #[cfg(test)]
        let _link_concurrency_guard =
            self.link_concurrency_probe.map(tests::LinkConcurrencyProbe::enter);

        let slot = open_slot(paths, global_slot)?;
        let marked_cas_paths = cas_paths_with_build_marker(
            self.cas_paths,
            &slot.paths.save_path,
            self.source.build_marker,
            SlotReimport {
                interrupted_build: slot.interrupted_build,
                force_import: self.source.force,
            },
        );
        let cas_paths = marked_cas_paths.as_ref().unwrap_or(self.cas_paths);

        self.import_and_link::<Reporter>(
            &slot.paths,
            cas_paths,
            slot.interrupted_build,
            slot.global_slot.as_ref(),
        )?;

        emit_imported::<Reporter>(&self, &slot.paths);
        Ok(())
    }

    fn import_and_link<Reporter: self::Reporter>(
        &self,
        slot: &SlotPaths,
        cas_paths: &pnpm_fs::CasPaths,
        interrupted_build: bool,
        global_slot: Option<&crate::GlobalVirtualStoreSlot>,
    ) -> Result<(), CreateVirtualDirError> {
        let import_package = || {
            self.import_slot::<Reporter>(&slot.save_path, cas_paths, interrupted_build, global_slot)
        };
        if self.dependencies.symlink {
            // `rayon::join` runs both closures in parallel on rayon's pool,
            // returning only once both finish. `import_indexed_dir` is itself
            // a rayon par_iter over CAS entries; `create_symlink_layout` is
            // a small serial loop over dep refs.
            let (cas_result, symlink_result) =
                rayon::join(import_package, || self.link_children(&slot.node_modules));
            cas_result?;
            symlink_result?;
            if !self.dependencies.include_optional {
                self.remove_optional_children(&slot.node_modules)?;
            }
        } else {
            import_package()?;
            self.remove_all_children(&slot.node_modules)?;
        }

        // Unlink children the package no longer depends on after the
        // package has materialized. The removed aliases are disjoint
        // from the package's own `node_modules/<self>` directory.
        remove_obsolete_children(
            &slot.node_modules,
            &self.dependencies.package_key.name,
            self.dependencies.removed_aliases,
        )?;

        Ok(())
    }

    fn import_method(&self) -> PackageImportMethod {
        self.import.method_for(self.source.is_mutable, self.source.needs_build)
    }

    fn import_slot<Reporter: self::Reporter>(
        &self,
        save_path: &Path,
        cas_paths: &pnpm_fs::CasPaths,
        interrupted_build: bool,
        global_slot: Option<&crate::GlobalVirtualStoreSlot>,
    ) -> Result<(), CreateVirtualDirError> {
        if !interrupted_build
            && !self.source.force
            && !self.source.is_mutable
            && let Some(slot) = global_slot
        {
            let populated = slot
                .is_populated()
                .map_err(|source| CreateVirtualDirError::ReadSlotState {
                    path: slot.path().to_path_buf(),
                    source,
                })?;
            if populated && marker_present(save_path, cas_paths) {
                return Ok(());
            }
        }
        // An interrupted build and a forced import both re-import with
        // `force`, which the cache's fresh-destination clone cannot serve.
        // For the forced one that is also a correctness bar, not just a
        // capability one: `DirCloneCache::canonical_slot_ready` materializes
        // the canonical slot with `force: false`, so cloning from it could
        // reproduce the very stale bytes `--force` is meant to replace. Both
        // callers already withhold the cache through `dir_clone_cacheable`;
        // repeating it here keeps the guarantee off a caller's memory.
        if self.try_import_cached::<Reporter>(save_path, cas_paths, interrupted_build)? {
            finish_slot_population(global_slot, cas_paths)?;
            return Ok(());
        }
        begin_slot_population(global_slot)?;
        self.import_indexed_directory::<Reporter>(save_path, cas_paths, interrupted_build)?;
        finish_slot_population(global_slot, cas_paths)
    }

    fn try_import_cached<Reporter: self::Reporter>(
        &self,
        save_path: &Path,
        cas_paths: &pnpm_fs::CasPaths,
        interrupted_build: bool,
    ) -> Result<bool, CreateVirtualDirError> {
        if interrupted_build || self.source.force {
            return Ok(false);
        }
        let Some(cache) = self.dir_clone_cache else {
            return Ok(false);
        };
        Ok(cache.try_import::<Reporter>(
            self.import.logged_methods,
            self.import_method(),
            self.dependencies.package_key,
            save_path,
            cas_paths,
        ))
    }

    fn import_indexed_directory<Reporter: self::Reporter>(
        &self,
        save_path: &Path,
        cas_paths: &pnpm_fs::CasPaths,
        interrupted_build: bool,
    ) -> Result<(), CreateVirtualDirError> {
        import_indexed_dir::<Reporter>(
            self.import.logged_methods,
            self.import_method(),
            save_path,
            cas_paths,
            slot_import_opts(
                self.layout,
                SlotForceInputs {
                    interrupted_build,
                    source_is_mutable: self.source.is_mutable,
                    source_exists: self.source.source_exists,
                    force_import: self.source.force,
                },
                self.import.swap_lock,
            ),
        )
        .map_err(CreateVirtualDirError::ImportIndexedDir)
    }

    fn link_children(&self, node_modules: &Path) -> Result<(), CreateVirtualDirError> {
        create_symlink_layout(
            self.dependencies.snapshot.dependencies.as_ref(),
            self.dependencies.snapshot.optional_dependencies.as_ref(),
            self.dependencies.include_optional,
            &self.dependencies.package_key.name,
            self.dependencies.skipped,
            self.layout,
            node_modules,
        )
        .map_err(CreateVirtualDirError::SymlinkPackage)
    }

    fn remove_optional_children(&self, node_modules: &Path) -> Result<(), CreateVirtualDirError> {
        remove_obsolete_children(
            node_modules,
            &self.dependencies.package_key.name,
            self.dependencies.snapshot.optional_dependencies
                .iter()
                .flatten()
                .map(|(alias, _)| alias),
        )
    }

    fn remove_all_children(&self, node_modules: &Path) -> Result<(), CreateVirtualDirError> {
        remove_obsolete_children(
            node_modules,
            &self.dependencies.package_key.name,
            self.dependencies.snapshot.dependencies
                .iter()
                .flat_map(|dependencies| dependencies.keys())
                .chain(
                    self.dependencies.snapshot.optional_dependencies
                        .iter()
                        .flat_map(|deps| deps.keys()),
                ),
        )
    }
}

fn begin_slot_population(
    slot: Option<&crate::GlobalVirtualStoreSlot>,
) -> Result<(), CreateVirtualDirError> {
    if let Some(slot) = slot {
        slot.begin_population()
            .map_err(|source| CreateVirtualDirError::WriteSlotState {
                path: slot.path().to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

fn finish_slot_population(
    slot: Option<&crate::GlobalVirtualStoreSlot>,
    cas_paths: &pnpm_fs::CasPaths,
) -> Result<(), CreateVirtualDirError> {
    if let Some(slot) = slot {
        slot.finish_population(if cas_paths.contains_key(NEEDS_BUILD_MARKER) {
            crate::BuildRequirement::Required
        } else {
            crate::BuildRequirement::NotRequired
        })
        .map_err(|source| CreateVirtualDirError::WriteSlotState {
            path: slot.path().to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn emit_imported<Reporter: self::Reporter>(
    installer: &CreateVirtualDirBySnapshot<'_>,
    slot: &SlotPaths,
) {
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Imported {
            method: optimistic_wire_method(installer.import_method()),
            requester: installer.import.requester.to_owned(),
            to: slot.save_path.to_string_lossy().into_owned(),
        },
    }));
}

/// Unlink every child but the package's own `node_modules/<self>` directory.
fn remove_obsolete_children<'a>(
    virtual_node_modules_dir: &Path,
    own_name: &PkgName,
    aliases: impl IntoIterator<Item = &'a PkgName>,
) -> Result<(), CreateVirtualDirError> {
    for alias in aliases {
        if alias != own_name {
            remove_obsolete_child(virtual_node_modules_dir, alias)?;
        }
    }
    Ok(())
}

/// The import method a slot's files are actually materialized with.
///
/// A package that a lifecycle script or a patch is still going to write must
/// not share inodes with the source its files were imported from: a hard link
/// carries those writes back into the workspace directory of an injected
/// package, or into the content-addressable store for a registry package.
/// `clone-or-copy` gives the build private inodes and still lets a reflink
/// avoid a byte-for-byte copy. An explicit `copy` already gives private
/// inodes, so it is kept. pnpm v11 applies the same override to
/// `willBeBuilt` packages in `createPackageImporter`.
#[must_use]
pub fn effective_import_method(
    configured: PackageImportMethod,
    needs_build: bool,
) -> PackageImportMethod {
    if needs_build && configured != PackageImportMethod::Copy {
        PackageImportMethod::CloneOrCopy
    } else {
        configured
    }
}

/// Map pacquet's configured [`PackageImportMethod`] to the value
/// `pnpm:progress imported`'s `method` field carries. pnpm only
/// distinguishes the three resolved methods.
#[must_use]
pub fn optimistic_wire_method(method: PackageImportMethod) -> WireImportMethod {
    match method {
        PackageImportMethod::Auto => crate::link_file::auto_optimistic_wire_method(),
        PackageImportMethod::Clone | PackageImportMethod::CloneOrCopy => WireImportMethod::Clone,
        PackageImportMethod::Hardlink => WireImportMethod::Hardlink,
        PackageImportMethod::Copy => WireImportMethod::Copy,
    }
}

/// Removes the links that point at optional snapshots whose fetch failed.
///
/// A parent's slot links its children while the downloads are still
/// running, so a link to a child that later fails to download would
/// otherwise be left dangling. Only links are removed: an alias that
/// names the parent itself, or any real directory, is left alone.
pub(crate) fn unlink_fetch_failed_children(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    fetch_failed: &HashSet<PackageKey>,
    layout: &VirtualStoreLayout,
) -> Result<(), CreateVirtualDirError> {
    for (key, snapshot) in snapshots {
        let children = snapshot.dependencies
            .iter()
            .chain(snapshot.optional_dependencies.iter())
            .flatten();
        let mut node_modules = None;
        for (alias, dep_ref) in children {
            if alias == &key.name
                || !dep_ref
                    .resolve(alias)
                    .is_some_and(|child| fetch_failed.contains(&child))
            {
                continue;
            }
            let node_modules =
                node_modules.get_or_insert_with(|| layout.slot_dir(key).join("node_modules"));
            let is_link = safe_join_modules_dir(node_modules, &alias.to_string())
                .is_ok_and(|child| is_symlink_or_junction(&child).unwrap_or(false));
            if is_link {
                remove_obsolete_child(node_modules, alias)?;
            }
        }
    }
    Ok(())
}

/// Unlink one obsolete child from a slot's `node_modules`.
///
/// Removes the `<node_modules>/<alias>` symlink and, for a scoped
/// alias, drops the now-empty `@scope` directory (ignoring the error
/// when another scoped sibling keeps it populated). `remove_symlink_dir`
/// unlinks the symlink itself, never its target package.
///
/// Invalid npm dependency names are ignored.
fn remove_obsolete_child(
    virtual_node_modules_dir: &Path,
    alias: &PkgName,
) -> Result<(), CreateVirtualDirError> {
    let Ok(child_path) = safe_join_modules_dir(virtual_node_modules_dir, &alias.to_string()) else {
        return Ok(());
    };
    if !is_subdir(virtual_node_modules_dir, &child_path) {
        return Ok(());
    }
    match remove_symlink_dir(&child_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CreateVirtualDirError::RemoveObsoleteChild { path: child_path, error });
        }
    }
    if let Some(scope) = &alias.scope {
        let _ = fs::remove_dir(virtual_node_modules_dir.join(format!("@{scope}")));
    }
    Ok(())
}

fn open_slot(
    slot: SlotPaths,
    global_slot: Option<crate::GlobalVirtualStoreSlot>,
) -> Result<OpenedSlot, CreateVirtualDirError> {
    slot.create_dirs()?;
    let interrupted_build = match &global_slot {
        Some(global_slot) => global_slot
            .has_build_marker()
            .map_err(|source| CreateVirtualDirError::ReadBuildMarker {
                path: slot.save_path.join(NEEDS_BUILD_MARKER),
                source,
            })?,
        None => slot.save_path.join(NEEDS_BUILD_MARKER).is_file(),
    };
    Ok(OpenedSlot { paths: slot, global_slot, interrupted_build })
}
