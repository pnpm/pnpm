pub(crate) mod specs;
pub(crate) use specs::{
    has_local_file_override, has_local_file_package_extension, is_local_file_spec,
    is_unambiguous_local_file_spec,
};
mod workspace;

use super::{
    CatalogAnchor, CatalogResolutionResult, Catalogs, DependencyGroup, Lockfile,
    OptimisticRepeatInstallCheck, Path, PathBuf, WantedDependency, resolve_from_catalog,
};
use pnpm_lockfile::{LockfileResolution, PkgName, is_local_tarball_path};
use pnpm_resolving_local_resolver::local_tarball_path;
use pnpm_workspace::importer_id_from_root_dir;
use ssri::Integrity;
use std::{borrow::Cow, collections::HashSet};

struct LocalTarballDependency {
    project_dir: PathBuf,
    alias: String,
    group: DependencyGroup,
    path: Option<PathBuf>,
    must_be_local: bool,
}

pub(crate) struct FrozenLocalTarballCheck<'a> {
    pub(crate) workspace_root: &'a Path,
    pub(crate) importer_ids: &'a HashSet<String>,
    pub(crate) groups: &'a crate::GroupSelection,
    pub(crate) lockfile: &'a Lockfile,
    pub(crate) skipped: &'a pnpm_deps_restorer::SkippedSnapshots,
}

impl FrozenLocalTarballCheck<'_> {
    fn package_keys(&self) -> HashSet<pnpm_lockfile::PackageKey> {
        crate::collect_reachable(
            self.lockfile,
            self.workspace_root,
            self.importer_ids,
            self.groups,
            |key| self.skipped.contains(key),
        )
        .snapshot_keys
    }
}

pub(crate) fn frozen_local_tarballs_to_verify(
    check: &FrozenLocalTarballCheck<'_>,
) -> Vec<(PathBuf, ssri::Integrity)> {
    let mut verified = HashSet::new();
    let mut targets = Vec::new();
    for key in check.package_keys() {
        let Some(metadata) = check.lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&key.without_peer()))
        else {
            continue;
        };
        let LockfileResolution::Tarball(resolution) = &metadata.resolution else { continue };
        if !is_local_tarball_path(&resolution.tarball) {
            continue;
        }
        let url = crate::local_file_tarball_install_url(
            Cow::Borrowed(&resolution.tarball),
            check.workspace_root,
        );
        let Some(recorded_path) = pnpm_tarball::local_file_tarball_path(&url) else {
            continue;
        };
        let Some(integrity) = resolution.integrity
            .as_ref()
            .filter(|value| !value.hashes.is_empty())
        else {
            continue;
        };
        if verified.insert((recorded_path.clone(), integrity.to_string())) {
            targets.push((recorded_path, integrity.clone()));
        }
    }
    targets
}

/// Whether any project declares a mutable local directory dependency or a
/// local tarball whose current bytes do not match the integrity recorded by
/// the previous install. Groups excluded from the current install are skipped.
/// `catalog:` specs are dereferenced through the workspace catalogs.
pub(crate) fn has_local_file_dep_requiring_install(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<bool, &'static str> {
    let tarballs = match scan_local_tarball_deps(check) {
        LocalTarballScan::RequiresInstall => return Ok(true),
        LocalTarballScan::Candidates(tarballs) => tarballs,
    };
    if tarballs.is_empty() {
        return Ok(false);
    }

    let current_lockfile;
    let lockfile = if let Some(lockfile) = check.lockfile
        .get()
        .map_err(|_| "the wanted lockfile cannot be loaded to verify local tarballs")?
    {
        lockfile
    } else {
        current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir)
                .map_err(|_| "the current lockfile cannot be loaded to verify local tarballs")?;
        let Some(lockfile) = current_lockfile.as_ref() else { return Ok(true) };
        lockfile
    };

    Ok(tarballs
        .iter()
        .any(|dependency| {
            local_tarball_requires_install(check.workspace_root, lockfile, dependency)
        }))
}

/// What the manifests' `file:` dependencies amount to.
enum LocalTarballScan {
    /// One of them names a path that cannot be resolved, which only an
    /// install can settle.
    RequiresInstall,
    Candidates(Vec<LocalTarballDependency>),
}

fn scan_local_tarball_deps(check: &OptimisticRepeatInstallCheck<'_>) -> LocalTarballScan {
    let fields: [(&str, DependencyGroup, bool); 3] = [
        ("dependencies", DependencyGroup::Prod, check.layout.included.dependencies),
        ("devDependencies", DependencyGroup::Dev, check.layout.included.dev_dependencies),
        (
            "optionalDependencies",
            DependencyGroup::Optional,
            check.layout.included.includes_project_optional_dependencies(),
        ),
    ];
    let workspace_packages = if check.config.inject_workspace_packages {
        workspace::collect_workspace_packages(check.project_manifests)
    } else {
        std::collections::HashMap::new()
    };
    let mut tarballs = Vec::new();
    for (project_dir, manifest) in check.project_manifests {
        if !scan_project_manifest_tarballs(
            check,
            &workspace_packages,
            project_dir,
            manifest,
            &fields,
            &mut tarballs,
        ) {
            return LocalTarballScan::RequiresInstall;
        }
    }
    LocalTarballScan::Candidates(tarballs)
}

fn scan_project_manifest_tarballs(
    check: &OptimisticRepeatInstallCheck<'_>,
    workspace_packages: &workspace::WorkspacePackageMap<'_>,
    project_dir: &Path,
    manifest: &pnpm_package_manifest::PackageManifest,
    fields: &[(&str, DependencyGroup, bool); 3],
    tarballs: &mut Vec<LocalTarballDependency>,
) -> bool {
    for (field, group, group_included) in fields {
        if !group_included {
            continue;
        }
        let scan = FieldTarballScan {
            catalogs: check.catalogs,
            workspace_dir: check.config.workspace_dir.as_deref(),
            project_dir,
            field,
            group: *group,
            inject_workspace_packages: check.config.inject_workspace_packages,
            workspace_packages,
        };
        if !scan_field_tarballs(&scan, manifest, tarballs) {
            return false;
        }
    }
    true
}

/// One manifest field of one project, as the tarball scan reads it.
struct FieldTarballScan<'a> {
    catalogs: &'a Catalogs,
    /// Where `pnpm-workspace.yaml` sits, so a `file:` catalog entry's
    /// relative path is measured from the same directory the install
    /// measures it from.
    workspace_dir: Option<&'a Path>,
    project_dir: &'a Path,
    field: &'a str,
    group: DependencyGroup,
    inject_workspace_packages: bool,
    workspace_packages: &'a workspace::WorkspacePackageMap<'a>,
}

/// `false` when a `file:` dependency in this field cannot be resolved to a
/// path.
fn scan_field_tarballs(
    scan: &FieldTarballScan<'_>,
    manifest: &pnpm_package_manifest::PackageManifest,
    tarballs: &mut Vec<LocalTarballDependency>,
) -> bool {
    let Some(deps) = manifest
        .value()
        .get(scan.field)
        .and_then(|value| value.as_object())
    else {
        return true;
    };
    for (alias, spec) in deps {
        if workspace::dependency_is_workspace_or_injected(
            scan.workspace_packages,
            scan.inject_workspace_packages,
            scan.catalogs,
            manifest.value(),
            alias,
            spec,
        ) {
            return false;
        }
        match local_tarball_candidate(scan, alias, spec) {
            LocalTarballCandidate::Skip => {}
            LocalTarballCandidate::Unresolvable => return false,
            LocalTarballCandidate::Found { path, must_be_local } => {
                tarballs.push(LocalTarballDependency {
                    project_dir: scan.project_dir.to_path_buf(),
                    alias: alias.clone(),
                    group: scan.group,
                    path,
                    must_be_local,
                });
            }
        }
    }
    true
}

/// What one declared dependency contributes to the tarball scan.
enum LocalTarballCandidate {
    /// Not a local `file:` dependency.
    Skip,
    Unresolvable,
    Found {
        path: Option<PathBuf>,
        must_be_local: bool,
    },
}

fn local_tarball_candidate(
    scan: &FieldTarballScan<'_>,
    alias: &str,
    spec: &serde_json::Value,
) -> LocalTarballCandidate {
    let Some(spec) = spec.as_str() else { return LocalTarballCandidate::Skip };
    let resolved_spec = resolve_catalog_spec(scan, alias, spec);
    let Some(spec) = resolved_spec.as_deref() else { return LocalTarballCandidate::Skip };
    if !is_local_file_spec(spec) {
        return LocalTarballCandidate::Skip;
    }
    let must_be_local = is_unambiguous_local_file_spec(spec);
    let path = local_tarball_path(spec, scan.project_dir);
    if must_be_local && path.is_none() {
        return LocalTarballCandidate::Unresolvable;
    }
    LocalTarballCandidate::Found { path, must_be_local }
}

fn resolve_catalog_spec<'a>(
    scan: &FieldTarballScan<'_>,
    alias: &str,
    spec: &'a str,
) -> Option<Cow<'a, str>> {
    if !spec.starts_with("catalog:") {
        return Some(Cow::Borrowed(spec));
    }
    match resolve_from_catalog(
        scan.catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
        match scan.workspace_dir {
            Some(workspace_dir) => {
                CatalogAnchor::Reanchor { workspace_dir, consumer_dir: Some(scan.project_dir) }
            }
            None => CatalogAnchor::AsWritten,
        },
    ) {
        CatalogResolutionResult::Found(found) => Some(Cow::Owned(found.resolution.specifier)),
        _ => None,
    }
}

fn local_tarball_requires_install(
    workspace_root: &Path,
    lockfile: &Lockfile,
    dependency: &LocalTarballDependency,
) -> bool {
    let importer_id = importer_id_from_root_dir(workspace_root, &dependency.project_dir);
    let resolution = match recorded_tarball(lockfile, &importer_id, dependency) {
        RecordedTarball::Missing => return true,
        RecordedTarball::NotATarball => return dependency.must_be_local,
        RecordedTarball::Tarball(resolution) => resolution,
    };
    if !resolution.tarball.starts_with("file:") {
        return dependency.must_be_local;
    }
    let Some(recorded_path) = local_tarball_path(&resolution.tarball, workspace_root) else {
        return true;
    };
    if dependency.path
        .as_ref()
        .is_some_and(|path| path != &recorded_path)
    {
        return true;
    }
    let Some(integrity) = resolution.integrity
        .as_ref()
        .filter(|value| !value.hashes.is_empty())
    else {
        return true;
    };
    !file_matches_integrity(&recorded_path, integrity)
}

/// What the lockfile records for a local tarball dependency.
enum RecordedTarball<'l> {
    /// No importer, alias or package entry: the dependency was never
    /// installed.
    Missing,
    NotATarball,
    Tarball(&'l pnpm_lockfile::TarballResolution),
}

fn recorded_tarball<'l>(
    lockfile: &'l Lockfile,
    importer_id: &str,
    dependency: &LocalTarballDependency,
) -> RecordedTarball<'l> {
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return RecordedTarball::Missing;
    };
    let Ok(alias) = PkgName::parse(&dependency.alias) else { return RecordedTarball::Missing };
    let Some(resolved) = importer
        .get_map_by_group(dependency.group)
        .and_then(|dependencies| dependencies.get(&alias))
    else {
        return RecordedTarball::Missing;
    };
    let Some(package_key) = resolved.version.resolved_key(&alias).map(|key| key.without_peer())
    else {
        return RecordedTarball::NotATarball;
    };
    let Some(metadata) = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&package_key))
    else {
        return RecordedTarball::Missing;
    };
    match &metadata.resolution {
        LockfileResolution::Tarball(resolution) => RecordedTarball::Tarball(resolution),
        _ => RecordedTarball::NotATarball,
    }
}

fn file_matches_integrity(path: &Path, integrity: &Integrity) -> bool {
    pnpm_tarball::verify_local_file_integrity(path, integrity).is_ok()
}
