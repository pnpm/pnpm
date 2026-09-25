use crate::{
    SymlinkPackageError,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
    symlink_package,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_lockfile::ProjectSnapshot;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{AddedRoot, DependencyType, LogEvent, LogLevel, RootLog, RootMessage};
use pnpm_resolving_resolver_base::WorkspacePackages;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Symlink direct dependencies omitted from the lockfile importer because
/// `excludeLinksFromLockfile` is enabled. Importer-owned aliases are left to
/// the lockfile materialization passes so their dedupe decisions are preserved.
pub fn link_manifest_link_deps<Reporter: pnpm_reporter::Reporter>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &PackageManifest)],
    importers: Option<&HashMap<String, ProjectSnapshot>>,
    workspace_packages: Option<&WorkspacePackages>,
    included: IncludedDependencies,
    modules_dir_name: &std::ffi::OsStr,
    link_options: &LinkBinsOptions,
) -> Result<(), LinkManifestLinkDepsError> {
    // The name must be a relative path of normal components, which
    // `Config::modules_dir_name` guarantees — but this helper is public,
    // and joined below it decides where symlinks (which force-replace
    // squatters) land, so it enforces the contract itself rather than
    // trusting every caller.
    let mut components = Path::new(modules_dir_name).components().peekable();
    let valid_name = components.peek().is_some()
        && components.all(|component| matches!(component, std::path::Component::Normal(_)));
    if !valid_name {
        return Err(LinkManifestLinkDepsError::InvalidModulesDirName {
            modules_dir_name: modules_dir_name.to_string_lossy().into_owned(),
        });
    }
    for (project_dir, manifest) in project_manifests {
        let importer_snapshot = importers.and_then(|importers| {
            importers.get(&pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        });
        let modules_dir = project_dir.join(modules_dir_name);
        let project = ProjectLinks {
            project_dir,
            modules_dir: &modules_dir,
            importer_snapshot,
            workspace_packages,
            included,
            link_options,
        };
        link_project_manifest_deps::<Reporter>(&project, manifest)?;
    }
    Ok(())
}

pub(crate) struct PruneManifestLinkDeps<'a> {
    pub(crate) workspace_root: &'a Path,
    pub(crate) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(crate) importers: Option<&'a HashMap<String, ProjectSnapshot>>,
    pub(crate) workspace_packages: Option<&'a WorkspacePackages>,
    pub(crate) previously_included: IncludedDependencies,
    pub(crate) new_included: IncludedDependencies,
    pub(crate) modules_dir_name: &'a std::ffi::OsStr,
    pub(crate) prunable_importer_ids: Option<&'a HashSet<String>>,
}

pub(crate) fn prune_manifest_link_deps(
    options: &PruneManifestLinkDeps<'_>,
) -> Result<(), pnpm_deps_restorer::PruneDirectDepsError> {
    let old_groups = pnpm_deps_restorer::selected_groups(options.previously_included);
    let new_groups = pnpm_deps_restorer::selected_groups(options.new_included);
    for (project_dir, manifest) in options.project_manifests {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(options.workspace_root, project_dir);
        if options.prunable_importer_ids.is_some_and(|ids| !ids.contains(&importer_id)) {
            continue;
        }
        let importer_snapshot = options.importers.and_then(|importers| importers.get(&importer_id));
        prune_project_manifest_link_deps(
            options,
            project_dir,
            manifest,
            importer_snapshot,
            &old_groups,
            &new_groups,
        )?;
    }
    Ok(())
}

fn prune_project_manifest_link_deps(
    options: &PruneManifestLinkDeps<'_>,
    project_dir: &Path,
    manifest: &PackageManifest,
    importer_snapshot: Option<&ProjectSnapshot>,
    old_groups: &[DependencyGroup],
    new_groups: &[DependencyGroup],
) -> Result<(), pnpm_deps_restorer::PruneDirectDepsError> {
    let new_names: HashSet<&str> = manifest
        .dependencies(new_groups.iter().copied())
        .map(|(alias, _)| alias)
        .collect();
    let modules_dir = project_dir.join(options.modules_dir_name);
    let Some(modules_dir) =
        pnpm_deps_restorer::confined_modules_dir(&modules_dir, options.workspace_root)
    else {
        return Ok(());
    };
    for (alias, spec) in manifest.dependencies(old_groups.iter().copied()) {
        if new_names.contains(alias) {
            continue;
        }
        if let Some(snapshot) = importer_snapshot
            && (snapshot_has_alias(snapshot, alias)
                || manifest_link_target(project_dir, options.workspace_packages, alias, spec)
                    .is_none())
        {
            continue;
        }
        pnpm_deps_restorer::remove_direct_dep_link(&modules_dir, alias)?;
    }
    Ok(())
}

/// One project's lockfile-excluded linked dependencies and where they are placed.
struct ProjectLinks<'a> {
    project_dir: &'a Path,
    modules_dir: &'a Path,
    importer_snapshot: Option<&'a ProjectSnapshot>,
    workspace_packages: Option<&'a WorkspacePackages>,
    included: IncludedDependencies,
    link_options: &'a LinkBinsOptions,
}

fn link_project_manifest_deps<Reporter: pnpm_reporter::Reporter>(
    project: &ProjectLinks<'_>,
    manifest: &PackageManifest,
) -> Result<(), LinkManifestLinkDepsError> {
    // Aliases this pass placed (created or already-correct), for
    // the bin-linking sweep below.
    let mut linked_aliases: Vec<String> = Vec::new();
    // Per-group iteration (instead of one flattened
    // `manifest.dependencies([...])` pass) so the `pnpm:root added`
    // event below carries the dependency's real group.
    for (included, group) in [
        (project.included.dependencies, DependencyGroup::Prod),
        (project.included.dev_dependencies, DependencyGroup::Dev),
        (project.included.includes_project_optional_dependencies(), DependencyGroup::Optional),
    ] {
        if !included {
            continue;
        }
        for (alias, spec) in manifest.dependencies([group]) {
            if link_manifest_dep::<Reporter>(project, group, alias, spec)? {
                // Bins are (re-)linked for reused symlinks too — the
                // `.bin` entry may be missing even when the package
                // link itself is already correct.
                linked_aliases.push(alias.to_string());
            }
        }
    }
    // Link the placed deps' declared bins into
    // `<modules_dir>/.bin`, matching v11's `linkDirectDeps` which
    // bin-linked every direct dep including `link:` ones. The
    // helper reads each manifest through the symlink and skips
    // targets without a `package.json` (Bit's manifest-less
    // component links), so a bin-less link is a no-op.
    if !linked_aliases.is_empty() {
        crate::link_direct_dep_bins(project.modules_dir, &linked_aliases, project.link_options)
            .map_err(LinkManifestLinkDepsError::LinkBins)?;
    }
    Ok(())
}

/// Place one manifest-linked dependency, answering whether it now owns a slot
/// in the project's `node_modules`.
fn link_manifest_dep<Reporter: pnpm_reporter::Reporter>(
    project: &ProjectLinks<'_>,
    group: DependencyGroup,
    alias: &str,
    spec: &str,
) -> Result<bool, LinkManifestLinkDepsError> {
    if project.importer_snapshot.is_some_and(|snapshot| snapshot_has_alias(snapshot, alias)) {
        return Ok(false);
    }
    let Some(target_path) =
        manifest_link_target(project.project_dir, project.workspace_packages, alias, spec)
    else {
        return Ok(false);
    };
    // The alias is a raw `package.json` object key — an
    // unvalidated string. Route the join through the same
    // package-name validity check the lockfile-driven
    // passes apply, so a crafted alias (`../.git`, an
    // absolute path, a backslash) cannot escape
    // `node_modules/`.
    let symlink_path = safe_join_modules_dir(project.modules_dir, alias)
        .map_err(LinkManifestLinkDepsError::InvalidAlias)?;
    let outcome = symlink_package(&target_path, &symlink_path)
        .map_err(|source| LinkManifestLinkDepsError::Symlink {
            alias: alias.to_string(),
            source,
        })?;
    if !outcome.reused {
        // `pnpm:root added`: mirror the lockfile-driven pass's
        // per-dependency emit so manifest-linked deps show up
        // in the `+N` summary and NDJSON output like
        // pnpm v11's `linkDirectDeps` reported them.
        Reporter::emit(&LogEvent::Root(RootLog {
            level: LogLevel::Debug,
            message: RootMessage::Added {
                prefix: project.project_dir.display().to_string(),
                added: AddedRoot {
                    name: alias.to_string(),
                    real_name: alias.to_string(),
                    version: Some(spec.to_string()),
                    dependency_type: Some(dependency_type_of(group)),
                    id: None,
                    latest: None,
                    linked_from: None,
                },
            },
        }));
    }
    Ok(true)
}

fn manifest_link_target(
    project_dir: &Path,
    workspace_packages: Option<&WorkspacePackages>,
    alias: &str,
    spec: &str,
) -> Option<PathBuf> {
    if let Some(target) = spec.strip_prefix("link:") {
        return Some(resolve_link_target(project_dir, target));
    }
    workspace_link_target(workspace_packages?, alias, spec)
}

pub(crate) fn workspace_link_target(
    workspace_packages: &WorkspacePackages,
    alias: &str,
    spec: &str,
) -> Option<PathBuf> {
    let parsed = pnpm_resolving_npm_resolver::parse_bare_specifier(
        spec,
        Some(alias),
        "latest",
        "https://registry.npmjs.org/",
    )?;
    if parsed.revision.is_some() {
        return None;
    }
    let versions = workspace_packages.get(&parsed.name)?;
    let version =
        pnpm_resolving_npm_resolver::pick_matching_local_version_or_null(versions, &parsed)?;
    versions.get(&version).map(pnpm_resolving_npm_resolver::resolve_workspace_package_dir)
}

fn dependency_type_of(group: DependencyGroup) -> DependencyType {
    match group {
        DependencyGroup::Prod => DependencyType::Prod,
        DependencyGroup::Dev => DependencyType::Dev,
        DependencyGroup::Optional => DependencyType::Optional,
        // The group list this pass iterates is peer-free.
        DependencyGroup::Peer => unreachable!("peers are not iterated by this pass"),
    }
}

/// `true` when the importer snapshot resolves `alias` in any of the
/// non-peer dependency groups — i.e. the lockfile knows the dep and
/// the lockfile-driven passes own its materialization.
pub(crate) fn snapshot_has_alias(snapshot: &ProjectSnapshot, alias: &str) -> bool {
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| snapshot.get_map_by_group(group))
        .any(|deps| {
            deps.keys()
                .any(|name| name.to_string() == alias)
        })
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
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_INVALID_MODULES_DIR_NAME))]
    InvalidModulesDirName {
        #[error(not(source))]
        modules_dir_name: String,
    },

    /// A dependency key that is not a valid npm package name — it
    /// would escape `node_modules/` (or collide with pnpm's layout)
    /// when joined as a directory name.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),

    /// Creating one manifest-linked dependency's symlink failed.
    #[display("Failed to link manifest-linked dependency {alias:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_LINK_MANIFEST_LINK_DEP_FAILED))]
    Symlink {
        alias: String,
        #[error(source)]
        source: SymlinkPackageError,
    },

    /// Linking the placed deps' bins into `<modules_dir>/.bin` failed.
    #[diagnostic(transparent)]
    LinkBins(#[error(source)] pnpm_cmd_shim::LinkBinsError),
}

#[cfg(test)]
mod tests;
