use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, NEEDS_BUILD_MARKER, SkippedSnapshots,
    SymlinkPackageError, VirtualStoreLayout, create_symlink_layout, import_indexed_dir,
    import_indexed_dir::marker_present,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::PackageImportMethod;
use pnpm_fs::{is_subdir, remove_symlink_dir};
use pnpm_lockfile::{PackageKey, PkgName, SnapshotEntry};
use pnpm_reporter::{
    LogEvent, LogLevel, PackageImportMethod as WireImportMethod, ProgressLog, ProgressMessage,
    Reporter,
};
use std::{
    collections::HashMap,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
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
    /// Per-install precomputed slot-directory mapping. The layout
    /// holds the root and knows how to resolve a per-snapshot slot
    /// (legacy `<root>/<flat-name>` vs GVS-shaped
    /// `<root>/<scope>/<name>/<version>/<hash>`) through a single
    /// [`VirtualStoreLayout::slot_dir`] lookup. See
    /// [`crate::VirtualStoreLayout`] for how it's built.
    pub layout: &'a VirtualStoreLayout,
    pub cas_paths: &'a HashMap<String, PathBuf>,
    pub import_method: PackageImportMethod,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See the comment on `link_file::log_method_once` for why this
    /// is install-scoped rather than module-static.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into `pnpm:progress` `imported`'s
    /// `requester`. Same value as the `prefix` in
    /// [`pnpm_reporter::StageLog`].
    pub requester: &'a str,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`.
    /// Currently unused by `imported` (whose payload doesn't carry
    /// `packageId`) but kept here so future progress channels (e.g.
    /// per-package counts) can read it without rethreading.
    pub package_id: &'a str,
    pub package_key: &'a PackageKey,
    pub snapshot: &'a SnapshotEntry,
    /// Whether this package's file map points at mutable local source
    /// (a `file:` / [`pnpm_lockfile::LockfileResolution::Directory`]
    /// resolution) rather than immutable CAS entries. pnpm's `file:` is
    /// a copy taken at install time — unlike `link:`, which symlinks —
    /// so the slot has to be rebuilt on every install: the source can
    /// change without the lockfile changing, and the completion-marker
    /// short-circuit in [`fn@crate::import_indexed_dir`] would otherwise
    /// leave the previous install's copy in place forever.
    pub source_is_mutable: bool,
    /// Whether links from the snapshot's `optionalDependencies` map
    /// participate in the slot layout.
    pub include_optional_dependencies: bool,
    /// Whether dependency links inside the slot should be created.
    /// `symlink: false` still imports the package itself but leaves its
    /// `node_modules` free of graph links for `PnP` resolution.
    pub symlink: bool,
    /// Snapshots whose slots were not materialized on this host —
    /// platform-mismatched optionals, `--no-optional` exclusions, and
    /// swallowed optional fetch failures. `create_symlink_layout`
    /// uses this to skip dangling symlinks to absent slots: an
    /// uninstallable optional snapshot is never linked.
    pub skipped: &'a SkippedSnapshots,
    /// Child aliases that were linked by a previous install but are no
    /// longer in this snapshot's dependency set. Their stale symlinks
    /// are unlinked from the slot before the progress event fires, so
    /// a warm reinstall that drops a dependency (e.g. via an override)
    /// doesn't leave a dangling child behind. Empty for fresh packages
    /// and for survivors whose dependency set only changed by addition.
    pub removed_aliases: &'a [PkgName],
    /// Empty source file imported as `.pnpm-needs-build` before the package's
    /// atomic completion marker when the package needs a build or patch.
    pub needs_build_marker_source: Option<&'a Path>,
    #[cfg(test)]
    pub link_concurrency_probe: Option<&'a tests::LinkConcurrencyProbe>,
}

/// The manifest every global virtual store slot carries, and what it is for.
///
/// A dependency's lifecycle scripts run in `<slot>/node_modules/<name>`. A
/// package that wants to know which project installed it reads the manifest
/// above that `node_modules` — the convention every git-hook installer
/// follows. In a project's own virtual store it finds the project; a slot is
/// shared by every project that resolves the same dependencies, so there is no
/// project there to find, and pnpm does not run a dependency's scripts on any
/// project's behalf. The read still has to resolve, or a package that merely
/// *looks* takes the install down with it
/// ([pnpm/pnpm#13318](https://github.com/pnpm/pnpm/issues/13318)) — so the slot
/// answers with a manifest that declares nothing.
///
/// Byte-identical to what the TypeScript CLI writes
/// (`writeVirtualStoreSlotManifest`), since both fill the same store.
const VIRTUAL_STORE_SLOT_MANIFEST: &str = "{\n  \"private\": true\n}\n";

/// Write [`VIRTUAL_STORE_SLOT_MANIFEST`] into a slot, once.
///
/// Concurrent installs materialize the same slot, so losing the race is
/// success — every writer has the same bytes. A failure is not worth failing an
/// install over either: the manifest only keeps a package that looks for its
/// consumer from crashing, and the install it would abort is the one that
/// still has every other reason to succeed.
fn write_slot_manifest(slot_dir: &Path) {
    let path = slot_dir.join("package.json");
    match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(VIRTUAL_STORE_SLOT_MANIFEST.as_bytes()) {
                tracing::warn!(target: "pnpm::store", ?error, path = %path.display(), "could not write the virtual store slot manifest");
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            tracing::warn!(target: "pnpm::store", ?error, path = %path.display(), "could not write the virtual store slot manifest");
        }
    }
}

/// Error type of [`CreateVirtualDirBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateVirtualDirError {
    #[display("Failed to recursively create node_modules directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_NODE_MODULES_DIR))]
    CreateNodeModulesDir {
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
        let CreateVirtualDirBySnapshot {
            layout,
            cas_paths,
            import_method,
            logged_methods,
            requester,
            package_id: _package_id,
            package_key,
            snapshot,
            source_is_mutable,
            include_optional_dependencies,
            symlink,
            skipped,
            removed_aliases,
            needs_build_marker_source,
            #[cfg(test)]
            link_concurrency_probe,
        } = self;

        #[cfg(test)]
        let _link_concurrency_guard =
            link_concurrency_probe.map(tests::LinkConcurrencyProbe::enter);

        let slot_dir = layout.slot_dir(package_key);
        let virtual_node_modules_dir = slot_dir.join("node_modules");
        fs::create_dir_all(&virtual_node_modules_dir).map_err(|error| {
            CreateVirtualDirError::CreateNodeModulesDir {
                dir: virtual_node_modules_dir.clone(),
                error,
            }
        })?;
        if layout.enable_global_virtual_store() {
            write_slot_manifest(&slot_dir);
        }

        let save_path =
            safe_join_modules_dir(&virtual_node_modules_dir, &package_key.name.to_string())
                .map_err(CreateVirtualDirError::InvalidAlias)?;

        let interrupted_build = save_path.join(NEEDS_BUILD_MARKER).is_file();
        let should_mark_build = needs_build_marker_source.is_some()
            && (interrupted_build || !marker_present(&save_path, cas_paths));
        let marked_cas_paths;
        let cas_paths = if should_mark_build {
            marked_cas_paths = {
                let mut paths = cas_paths.clone();
                paths.insert(
                    NEEDS_BUILD_MARKER.to_string(),
                    needs_build_marker_source.expect("checked above").to_path_buf(),
                );
                paths
            };
            &marked_cas_paths
        } else {
            cas_paths
        };
        // Mutable sources can reuse a slot for different contents, so a complete import may be stale.
        let safe_to_skip = layout.enable_global_virtual_store() && !source_is_mutable;
        let import_opts = if interrupted_build || source_is_mutable {
            ImportIndexedDirOpts { force: true, keep_modules_dir: true, safe_to_skip }
        } else {
            ImportIndexedDirOpts { safe_to_skip, ..ImportIndexedDirOpts::default() }
        };

        let import_package = || {
            import_indexed_dir::<Reporter>(
                logged_methods,
                import_method,
                &save_path,
                cas_paths,
                import_opts,
            )
            .map_err(CreateVirtualDirError::ImportIndexedDir)
        };
        if symlink {
            // `rayon::join` runs both closures in parallel on rayon's pool,
            // returning only once both finish. `import_indexed_dir` is itself
            // a rayon par_iter over CAS entries; `create_symlink_layout` is
            // a small serial loop over dep refs.
            let (cas_result, symlink_result) = rayon::join(import_package, || {
                create_symlink_layout(
                    snapshot.dependencies.as_ref(),
                    snapshot.optional_dependencies.as_ref(),
                    include_optional_dependencies,
                    &package_key.name,
                    skipped,
                    layout,
                    &virtual_node_modules_dir,
                )
                .map_err(CreateVirtualDirError::SymlinkPackage)
            });
            cas_result?;
            symlink_result?;
            if !include_optional_dependencies {
                for alias in snapshot.optional_dependencies.iter().flatten().map(|(alias, _)| alias)
                {
                    if *alias != package_key.name {
                        remove_obsolete_child(&virtual_node_modules_dir, alias)?;
                    }
                }
            }
        } else {
            import_package()?;
            for alias in
                snapshot.dependencies.iter().flat_map(|dependencies| dependencies.keys()).chain(
                    snapshot
                        .optional_dependencies
                        .iter()
                        .flat_map(|dependencies| dependencies.keys()),
                )
            {
                if *alias != package_key.name {
                    remove_obsolete_child(&virtual_node_modules_dir, alias)?;
                }
            }
        }

        // Unlink children the package no longer depends on after the
        // package has materialized. The removed aliases are disjoint
        // from the package's own `node_modules/<self>` directory.
        for alias in removed_aliases {
            if *alias == package_key.name {
                continue;
            }
            remove_obsolete_child(&virtual_node_modules_dir, alias)?;
        }

        // `pnpm:progress imported` fires one event per (resolved +
        // fetched) package once its CAFS import has finished. `to` is
        // the per-package directory
        // inside the virtual store. `method` is best-effort — pacquet
        // doesn't surface the per-package resolved method past
        // `link_file`'s install-scoped atomic, so we report the
        // optimistic value the configured method would resolve to in
        // a non-degraded environment (`Auto` → its platform ladder's
        // head, `CloneOrCopy` → `clone`, explicit settings as-is).
        // Refining to per-package resolution
        // would require threading the resolved method back from
        // `link_file`; tracked under <https://github.com/pnpm/pacquet/issues/347>.
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::Imported {
                method: optimistic_wire_method(import_method),
                requester: requester.to_owned(),
                to: save_path.to_string_lossy().into_owned(),
            },
        }));

        Ok(())
    }
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
/// `is_subdir` is the traversal guard: `PkgName` parsing accepts shapes
/// such as `..` that would resolve outside the slot, so an alias that
/// doesn't stay within `node_modules` is skipped rather than removed.
fn remove_obsolete_child(
    virtual_node_modules_dir: &Path,
    alias: &PkgName,
) -> Result<(), CreateVirtualDirError> {
    let child_path = virtual_node_modules_dir.join(alias.to_string());
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

#[cfg(test)]
pub mod tests;
