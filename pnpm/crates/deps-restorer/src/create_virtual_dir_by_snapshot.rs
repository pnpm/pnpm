#[cfg(test)]
pub mod tests;

use crate::{
    DirCloneCache, ImportIndexedDirError, ImportIndexedDirOpts, NEEDS_BUILD_MARKER,
    SymlinkPackageError, VirtualStoreLayout, create_symlink_layout, import_indexed_dir,
    import_indexed_dir::marker_present,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::PackageImportMethod;
use pnpm_fs::{is_subdir, remove_symlink_dir};
use pnpm_lockfile::{PackageKey, PkgName};
use pnpm_reporter::{
    LogEvent, LogLevel, PackageImportMethod as WireImportMethod, ProgressLog, ProgressMessage,
    Reporter,
};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
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
    pub cas_paths: &'a HashMap<String, PathBuf>,
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

/// Error type of [`CreateVirtualDirBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateVirtualDirError {
    #[display("Failed to create node_modules directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_NODE_MODULES_DIR))]
    CreateNodeModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create virtual store slot directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_SLOT_DIR))]
    CreateSlotDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    SymlinkPackage(#[error(source)] SymlinkPackageError),

    /// The snapshot's package name is not a valid npm package name, so
    /// joining it under the slot's `node_modules` could escape the
    /// directory. Surfaces pnpm's `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),

    #[display("Failed to remove obsolete child link at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_REMOVE_OBSOLETE_CHILD))]
    RemoveObsoleteChild {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

impl CreateVirtualDirBySnapshot<'_> {
    /// Execute the subroutine.
    pub fn run<Reporter: self::Reporter>(self) -> Result<(), CreateVirtualDirError> {
        #[cfg(test)]
        let _link_concurrency_guard =
            self.link_concurrency_probe.map(tests::LinkConcurrencyProbe::enter);

        let slot = SlotPaths::create(self.layout, self.dependencies.package_key)?;
        let interrupted_build = slot.save_path.join(NEEDS_BUILD_MARKER).is_file();
        let marked_cas_paths = cas_paths_with_build_marker(
            self.cas_paths,
            &slot.save_path,
            self.source.build_marker,
            (interrupted_build, self.source.force),
        );
        let cas_paths = marked_cas_paths.as_ref().unwrap_or(self.cas_paths);

        let import_package =
            || self.import_slot::<Reporter>(&slot.save_path, cas_paths, interrupted_build);
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

        // `pnpm:progress imported` fires one event per (resolved +
        // fetched) package once its CAFS import has finished. `to` is
        // the per-package directory
        // inside the virtual store. `method` is best-effort — pacquet
        // doesn't surface the per-package resolved method past
        // `link_file`'s install-scoped atomic, so we report the
        // optimistic value this slot's import method would resolve to in
        // a non-degraded environment (`Auto` → its platform ladder's
        // head, `CloneOrCopy` → `clone`, explicit settings as-is).
        // Refining to per-package resolution
        // would require threading the resolved method back from
        // `link_file`; tracked under <https://github.com/pnpm/pacquet/issues/347>.
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::Imported {
                method: optimistic_wire_method(self.import_method()),
                requester: self.import.requester.to_owned(),
                to: slot.save_path.to_string_lossy().into_owned(),
            },
        }));

        Ok(())
    }

    /// The method this slot's files are imported with — the configured one,
    /// unless a build or patch is still going to write them.
    fn import_method(&self) -> PackageImportMethod {
        effective_import_method(self.import.method, self.source.needs_build)
    }

    fn import_slot<Reporter: self::Reporter>(
        &self,
        save_path: &Path,
        cas_paths: &HashMap<String, PathBuf>,
        interrupted_build: bool,
    ) -> Result<(), CreateVirtualDirError> {
        // An interrupted build and a forced import both re-import with
        // `force`, which the cache's fresh-destination clone cannot serve.
        // For the forced one that is also a correctness bar, not just a
        // capability one: `DirCloneCache::canonical_slot_ready` materializes
        // the canonical slot with `force: false`, so cloning from it could
        // reproduce the very stale bytes `--force` is meant to replace. Both
        // callers already withhold the cache through `dir_clone_cacheable`;
        // repeating it here keeps the guarantee off a caller's memory.
        if !interrupted_build
            && !self.source.force
            && let Some(cache) = self.dir_clone_cache
            && cache.try_import::<Reporter>(
                self.import.logged_methods,
                self.import_method(),
                self.dependencies.package_key,
                save_path,
                cas_paths,
            )
        {
            return Ok(());
        }
        import_indexed_dir::<Reporter>(
            self.import.logged_methods,
            self.import_method(),
            save_path,
            cas_paths,
            slot_import_opts(
                self.layout,
                (interrupted_build, self.source.is_mutable, self.source.force),
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

/// The slot's directories, created.
struct SlotPaths {
    node_modules: PathBuf,
    save_path: PathBuf,
}

impl SlotPaths {
    fn create(
        layout: &VirtualStoreLayout,
        package_key: &PackageKey,
    ) -> Result<Self, CreateVirtualDirError> {
        let slot_dir = layout.slot_dir(package_key);
        let node_modules = slot_dir.join("node_modules");
        // Two direct `mkdir`s instead of one `create_dir_all` on the
        // deepest path: the recursive form probes bottom-up with a
        // failing `mkdir` per missing ancestor before creating them
        // top-down, which on the APFS-serialized metadata path costs a
        // large install ~3 extra syscalls per slot. The virtual-store
        // root exists (steady state) — only its absence falls back to
        // the recursive form.
        create_slot_dirs(&slot_dir, &node_modules)?;
        let save_path = safe_join_modules_dir(&node_modules, &package_key.name.to_string())
            .map_err(CreateVirtualDirError::InvalidAlias)?;
        Ok(Self { node_modules, save_path })
    }
}

/// Two direct `mkdir`s instead of one `create_dir_all` on the deepest path:
/// the recursive form probes bottom-up with a failing `mkdir` per missing
/// ancestor before creating them top-down, which on the APFS-serialized
/// metadata path costs a large install ~3 extra syscalls per slot. The
/// virtual-store root exists (steady state) — only its absence falls back to
/// the recursive form.
fn create_slot_dirs(
    slot_dir: &Path,
    virtual_node_modules_dir: &Path,
) -> Result<(), CreateVirtualDirError> {
    match fs::create_dir(slot_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(slot_dir)
                .map_err(|error| CreateVirtualDirError::CreateSlotDir {
                    dir: slot_dir.to_path_buf(),
                    error,
                })?;
        }
        Err(error) => {
            return Err(CreateVirtualDirError::CreateSlotDir {
                dir: slot_dir.to_path_buf(),
                error,
            });
        }
    }
    match fs::create_dir(virtual_node_modules_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(CreateVirtualDirError::CreateNodeModulesDir {
            dir: virtual_node_modules_dir.to_path_buf(),
            error,
        }),
    }
}

/// The CAS paths plus a `.pnpm-needs-build` marker, when the slot has to carry
/// one it does not already have.
///
/// A finished slot's completion marker is normally proof that its build
/// ran too, so the marker is left out. Two cases break that and have to
/// carry it anyway: a build this install interrupted, and a forced
/// re-import, which replaces the slot's files with the pristine base map
/// and so undoes whatever the build did to them. Without the marker
/// [`slot_carries_overlay`](crate::build_modules::slot_carries_overlay) would read the re-imported files as a
/// cache hit and skip the rebuild.
fn cas_paths_with_build_marker(
    cas_paths: &HashMap<String, PathBuf>,
    save_path: &Path,
    needs_build_marker_source: Option<&Path>,
    forced: (bool, bool),
) -> Option<HashMap<String, PathBuf>> {
    let (interrupted_build, force_import) = forced;
    let source = needs_build_marker_source?;
    if !interrupted_build && !force_import && marker_present(save_path, cas_paths) {
        return None;
    }
    let mut paths = cas_paths.clone();
    paths.insert(NEEDS_BUILD_MARKER.to_string(), source.to_path_buf());
    Some(paths)
}

fn slot_import_opts(
    layout: &crate::VirtualStoreLayout,
    slot: (bool, bool, bool),
) -> ImportIndexedDirOpts {
    let (interrupted_build, source_is_mutable, force_import) = slot;
    // Mutable sources can reuse a slot for different contents, so a complete
    // import may be stale.
    let safe_to_skip = layout.enable_global_virtual_store() && !source_is_mutable;
    if interrupted_build || source_is_mutable || force_import {
        return ImportIndexedDirOpts {
            force: true,
            keep_modules_dir: true,
            safe_to_skip,
            preserve_symlinks: source_is_mutable,
        };
    }
    ImportIndexedDirOpts { safe_to_skip, ..ImportIndexedDirOpts::default() }
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
/// avoid a byte-for-byte copy. pnpm v11 applies the same override to
/// `willBeBuilt` packages in `createPackageImporter`.
#[must_use]
pub fn effective_import_method(
    configured: PackageImportMethod,
    needs_build: bool,
) -> PackageImportMethod {
    if needs_build { PackageImportMethod::CloneOrCopy } else { configured }
}

/// Map pacquet's configured [`PackageImportMethod`] to the value
/// `pnpm:progress imported`'s `method` field carries. pnpm only
/// distinguishes the three resolved methods.
/// See the comment at the emit site for why this is best-effort.
#[must_use]
pub fn optimistic_wire_method(method: PackageImportMethod) -> WireImportMethod {
    match method {
        PackageImportMethod::Auto => crate::link_file::auto_optimistic_wire_method(),
        PackageImportMethod::Clone | PackageImportMethod::CloneOrCopy => WireImportMethod::Clone,
        PackageImportMethod::Hardlink => WireImportMethod::Hardlink,
        PackageImportMethod::Copy => WireImportMethod::Copy,
    }
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
