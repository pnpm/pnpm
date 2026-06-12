use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, SkippedSnapshots, SymlinkPackageError,
    VirtualStoreLayout, create_symlink_layout, import_indexed_dir,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::PackageImportMethod;
use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_reporter::{
    LogEvent, LogLevel, PackageImportMethod as WireImportMethod, ProgressLog, ProgressMessage,
    Reporter,
};
use std::{collections::HashMap, fs, io, path::PathBuf, sync::atomic::AtomicU8};

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
    /// Per-install precomputed slot-directory mapping. Replaces the
    /// previous `virtual_store_dir: &Path` field — the layout already
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
    /// [`pacquet_reporter::StageLog`].
    pub requester: &'a str,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`.
    /// Currently unused by `imported` (whose payload doesn't carry
    /// `packageId`) but kept here so future progress channels (e.g.
    /// per-package counts) can read it without rethreading.
    pub package_id: &'a str,
    pub package_key: &'a PackageKey,
    pub snapshot: &'a SnapshotEntry,
    /// Snapshots whose slots were not materialized on this host —
    /// platform-mismatched optionals, `--no-optional` exclusions, and
    /// swallowed optional fetch failures. `create_symlink_layout`
    /// uses this to skip dangling symlinks to absent slots. Mirrors
    /// upstream's `!pkg.installable && pkg.optional` short-circuit in
    /// `linkAllModules` at
    /// <https://github.com/pnpm/pnpm/blob/f2981a316/installing/deps-installer/src/install/link.ts#L540>.
    pub skipped: &'a SkippedSnapshots,
    #[cfg(test)]
    pub(crate) link_concurrency_probe: Option<&'a tests::LinkConcurrencyProbe>,
}

/// Error type of [`CreateVirtualDirBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateVirtualDirError {
    #[display("Failed to recursively create node_modules directory at {dir:?}: {error}")]
    #[diagnostic(code(pacquet_package_manager::create_node_modules_dir))]
    CreateNodeModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    SymlinkPackage(#[error(source)] SymlinkPackageError),
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
            skipped,
            #[cfg(test)]
            link_concurrency_probe,
        } = self;

        #[cfg(test)]
        let _link_concurrency_guard =
            link_concurrency_probe.map(tests::LinkConcurrencyProbe::enter);

        let virtual_node_modules_dir = layout.slot_dir(package_key).join("node_modules");
        fs::create_dir_all(&virtual_node_modules_dir).map_err(|error| {
            CreateVirtualDirError::CreateNodeModulesDir {
                dir: virtual_node_modules_dir.clone(),
                error,
            }
        })?;

        let save_path = virtual_node_modules_dir.join(package_key.name.to_string());

        // `rayon::join` runs both closures in parallel on rayon's pool,
        // returning only once both finish. `import_indexed_dir` is itself
        // a rayon par_iter over CAS entries; `create_symlink_layout` is
        // a small serial loop over dep refs. Overlapping them saves the
        // symlink time from the per-snapshot critical path without any
        // cross-thread data marshaling — both closures borrow from the
        // current stack frame.
        let (cas_result, symlink_result) = rayon::join(
            || {
                import_indexed_dir::<Reporter>(
                    logged_methods,
                    import_method,
                    &save_path,
                    cas_paths,
                    ImportIndexedDirOpts::default(),
                )
                .map_err(CreateVirtualDirError::ImportIndexedDir)
            },
            || {
                create_symlink_layout(
                    snapshot.dependencies.as_ref(),
                    snapshot.optional_dependencies.as_ref(),
                    &package_key.name,
                    skipped,
                    layout,
                    &virtual_node_modules_dir,
                )
                .map_err(CreateVirtualDirError::SymlinkPackage)
            },
        );
        cas_result?;
        symlink_result?;

        // `pnpm:progress imported` mirrors pnpm's emit at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/link.ts#L498>:
        // one event per (resolved + fetched) package once its CAFS
        // import has finished. `to` is the per-package directory
        // inside the virtual store. `method` is best-effort — pacquet
        // doesn't surface the per-package resolved method past
        // `link_file`'s install-scoped atomic, so we report the
        // optimistic value the configured method would resolve to in
        // a non-degraded environment (`Auto`/`CloneOrCopy` → `clone`,
        // explicit settings as-is). Refining to per-package resolution
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
/// distinguishes the three resolved methods; for `Auto` and
/// `CloneOrCopy` the optimistic first-attempt method is `clone`.
/// See the comment at the emit site for why this is best-effort.
pub(crate) fn optimistic_wire_method(method: PackageImportMethod) -> WireImportMethod {
    match method {
        PackageImportMethod::Auto
        | PackageImportMethod::Clone
        | PackageImportMethod::CloneOrCopy => WireImportMethod::Clone,
        PackageImportMethod::Hardlink => WireImportMethod::Hardlink,
        PackageImportMethod::Copy => WireImportMethod::Copy,
    }
}

#[cfg(test)]
pub(crate) mod tests;
