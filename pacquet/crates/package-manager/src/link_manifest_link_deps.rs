use crate::{
    SymlinkPackageError,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
    symlink_package,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::ProjectSnapshot;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{AddedRoot, DependencyType, LogEvent, LogLevel, RootLog, RootMessage};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Symlink each project's lockfile-excluded `link:`-spec dependencies
/// into its `node_modules/`, sourced from the in-memory project
/// manifests.
///
/// `excludeLinksFromLockfile` strips non-`workspace:` `link:` direct
/// deps from the lockfile importers, so the lockfile-driven
/// [`crate::SymlinkDirectDependencies`] pass never sees them. pnpm
/// v11's `linkDirectDeps` worked from the projects' own manifests and
/// materialized them regardless of the lockfile shape; without this
/// pass a project whose runtime is provided through `link:` specs —
/// Bit's capsule installs link the running binary's `@teambit/*`
/// aspects this way, and its `nmSelfReferences` adds a `link:.`
/// self-reference — ends up with those entries silently missing.
///
/// An alias the project's lockfile importer *does* carry is skipped
/// entirely: the lockfile pass owns it, including its
/// `dedupeDirectDeps` decision (re-linking it here would undo a
/// dedupe). Only aliases absent from the importer snapshot — the
/// excluded links — are filled in from the manifest.
///
/// Idempotent: an existing symlink at the alias path is force-replaced
/// (matching v11's re-link semantics), and `force_symlink_dir` creates
/// missing parent directories on demand. Specs that don't start with
/// `link:` are ignored — everything else is the lockfile passes' job.
pub fn link_manifest_link_deps<Reporter: pacquet_reporter::Reporter>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &PackageManifest)],
    importers: Option<&HashMap<String, ProjectSnapshot>>,
    modules_dir_name: &std::ffi::OsStr,
) -> Result<(), LinkManifestLinkDepsError> {
    // The name must be a single normal path component. The install
    // call site derives it from `config.modules_dir.file_name()`,
    // which by construction never yields `.`, `..`, or a separator —
    // but this helper is public, and joined below it decides where
    // symlinks (which force-replace squatters) land, so it enforces
    // the contract itself rather than trusting every caller.
    let valid_name = matches!(
        Path::new(modules_dir_name).components().collect::<Vec<_>>().as_slice(),
        [std::path::Component::Normal(_)],
    );
    if !valid_name {
        return Err(LinkManifestLinkDepsError::InvalidModulesDirName {
            modules_dir_name: modules_dir_name.to_string_lossy().into_owned(),
        });
    }
    for (project_dir, manifest) in project_manifests {
        let importer_snapshot = importers.and_then(|importers| {
            importers
                .get(&pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        });
        // The per-project modules dir honors a `modulesDir` override
        // the same way `SymlinkDirectDependencies` does — the caller
        // passes `config.modules_dir`'s basename, so a
        // `modulesDir: custom_modules` config doesn't grow a stray
        // `node_modules/` next to the intended tree.
        let modules_dir = project_dir.join(modules_dir_name);
        // Aliases this pass placed (created or already-correct), for
        // the bin-linking sweep below.
        let mut linked_aliases: Vec<String> = Vec::new();
        // Per-group iteration (instead of one flattened
        // `manifest.dependencies([...])` pass) so the `pnpm:root added`
        // event below carries the dependency's real group.
        for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
            for (alias, spec) in manifest.dependencies([group]) {
                let Some(target) = spec.strip_prefix("link:") else {
                    continue;
                };
                if importer_snapshot.is_some_and(|snapshot| snapshot_has_alias(snapshot, alias)) {
                    continue;
                }
                // The alias is a raw `package.json` object key — an
                // unvalidated string. Route the join through the same
                // package-name validity check the lockfile-driven
                // passes apply, so a crafted alias (`../.git`, an
                // absolute path, a backslash) cannot escape
                // `node_modules/`.
                let symlink_path = safe_join_modules_dir(&modules_dir, alias)
                    .map_err(LinkManifestLinkDepsError::InvalidAlias)?;
                let target_path = resolve_link_target(project_dir, target);
                let outcome = symlink_package(&target_path, &symlink_path).map_err(|source| {
                    LinkManifestLinkDepsError::Symlink { alias: alias.to_string(), source }
                })?;
                // Bins are (re-)linked for reused symlinks too — the
                // `.bin` entry may be missing even when the package
                // link itself is already correct.
                linked_aliases.push(alias.to_string());
                if outcome.reused {
                    continue;
                }
                // `pnpm:root added`: mirror the lockfile-driven pass's
                // per-dependency emit so manifest-linked deps show up
                // in the `+N` summary and NDJSON output like
                // pnpm v11's `linkDirectDeps` reported them.
                Reporter::emit(&LogEvent::Root(RootLog {
                    level: LogLevel::Debug,
                    message: RootMessage::Added {
                        prefix: project_dir.display().to_string(),
                        added: AddedRoot {
                            name: alias.to_string(),
                            real_name: alias.to_string(),
                            version: Some(spec.to_string()),
                            dependency_type: Some(match group {
                                DependencyGroup::Prod => DependencyType::Prod,
                                DependencyGroup::Dev => DependencyType::Dev,
                                DependencyGroup::Optional => DependencyType::Optional,
                                // The group list above is peer-free.
                                DependencyGroup::Peer => {
                                    unreachable!("peers are not iterated by this pass")
                                }
                            }),
                            id: None,
                            latest: None,
                            linked_from: None,
                        },
                    },
                }));
            }
        }
        // Link the placed deps' declared bins into
        // `<modules_dir>/.bin`, matching v11's `linkDirectDeps` which
        // bin-linked every direct dep including `link:` ones. The
        // helper reads each manifest through the symlink and skips
        // targets without a `package.json` (Bit's manifest-less
        // component links), so a bin-less link is a no-op.
        if !linked_aliases.is_empty() {
            crate::link_direct_dep_bins(&modules_dir, &linked_aliases)
                .map_err(LinkManifestLinkDepsError::LinkBins)?;
        }
    }
    Ok(())
}

/// `true` when the importer snapshot resolves `alias` in any of the
/// non-peer dependency groups — i.e. the lockfile knows the dep and
/// the lockfile-driven passes own its materialization.
fn snapshot_has_alias(snapshot: &ProjectSnapshot, alias: &str) -> bool {
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| snapshot.get_map_by_group(group))
        .any(|deps| deps.keys().any(|name| name.to_string() == alias))
}

/// Resolve a `link:` payload against the project directory. An
/// absolute payload is used as-is; a relative one (including the
/// self-reference `link:.`) is anchored at the project dir — the same
/// semantics pnpm applies to `link:` specifiers in a manifest.
fn resolve_link_target(project_dir: &Path, target: &str) -> PathBuf {
    let path = Path::new(target);
    if path.is_absolute() { path.to_path_buf() } else { project_dir.join(path) }
}

/// Error type of [`link_manifest_link_deps`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkManifestLinkDepsError {
    /// The modules-dir name is not a single normal path component
    /// (`.`, `..`, empty, absolute, or contains a separator) — joined
    /// under a project dir it would place symlinks outside the
    /// intended modules directory.
    #[display("Refusing to link into invalid modules directory name {modules_dir_name:?}")]
    #[diagnostic(code(pacquet_package_manager::invalid_modules_dir_name))]
    InvalidModulesDirName {
        #[error(not(source))]
        modules_dir_name: String,
    },

    /// A dependency key that is not a valid npm package name — it
    /// would escape `node_modules/` (or collide with pnpm's layout)
    /// when joined as a directory name.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),

    /// Creating one `link:` dep's symlink failed (permission denied,
    /// a real directory squatting the alias path, disk full, ...).
    #[display("Failed to link manifest `link:` dependency {alias:?}: {source}")]
    #[diagnostic(code(pacquet_package_manager::link_manifest_link_dep_failed))]
    Symlink {
        alias: String,
        #[error(source)]
        source: SymlinkPackageError,
    },

    /// Linking the placed deps' bins into `<modules_dir>/.bin` failed.
    #[diagnostic(transparent)]
    LinkBins(#[error(source)] pacquet_cmd_shim::LinkBinsError),
}

#[cfg(test)]
mod tests;
