pub(super) use packages::LinkedPackages;

use super::{
    BadPeerIssue, CatalogAnchor, CatalogResolutionError, CatalogResolutionResult, Catalogs,
    LinkedDependency, LinkedLockfileLoader, Lockfile, LockfileResolution, MissingPeerIssue,
    PackageManifest, ParentPkg, Path, PathBuf, PeerIssues, PkgName, ProjectManifests,
    ProjectSnapshot, ResolvedDependencySpec, WantedDependency, extract_peer_version,
    get_peer_version_range, resolve_from_catalog, satisfies,
};
use packages::linked_manifest;
use std::sync::OnceLock;

mod packages;

pub(super) struct LinkRoots<'a> {
    pub(super) lockfile: &'a Path,
    boundary: Option<&'a Path>,
    canonical_lockfile: OnceLock<Option<PathBuf>>,
    canonical_boundary: OnceLock<Option<PathBuf>>,
}

impl<'a> LinkRoots<'a> {
    pub(super) fn new(lockfile: &'a Path, workspace: Option<&'a Path>) -> Self {
        Self {
            lockfile,
            boundary: workspace.filter(|dir| *dir != lockfile),
            canonical_lockfile: OnceLock::new(),
            canonical_boundary: OnceLock::new(),
        }
    }

    pub(super) fn same(lockfile: &'a Path) -> Self {
        Self::new(lockfile, None)
    }

    pub(super) fn canonical_lockfile_dir(&self) -> Option<&Path> {
        self.canonical_lockfile
            .get_or_init(|| dunce::canonicalize(self.lockfile).ok())
            .as_deref()
    }

    fn canonical_boundary_dir(&self) -> Option<&Path> {
        let boundary = self.boundary?;
        self.canonical_boundary
            .get_or_init(|| dunce::canonicalize(boundary).ok())
            .as_deref()
    }

    pub(super) fn canonical_target(&self, path: &Path) -> Option<PathBuf> {
        if self.canonical_lockfile_dir().is_none() && self.canonical_boundary_dir().is_none() {
            return None;
        }
        let canonical_path = dunce::canonicalize(path).ok()?;
        let within_lockfile = self
            .canonical_lockfile_dir()
            .is_some_and(|lockfile_dir| canonical_path.starts_with(lockfile_dir));
        let allowed = within_lockfile
            || self
                .canonical_boundary_dir()
                .is_some_and(|boundary| canonical_path.starts_with(boundary));
        allowed.then_some(canonical_path)
    }
}

/// `base_dir` is the directory the `link:` target is relative to — the
/// importer's directory for importer dependencies, the lockfile directory for
/// snapshot dependencies. Targets escaping `roots` are rejected.
pub(super) fn resolve_link_version(
    base_dir: &Path,
    roots: &LinkRoots<'_>,
    link_target: &str,
) -> Option<String> {
    let target_dir = roots.canonical_target(&base_dir.join(link_target))?;
    let manifest = pnpm_workspace::read_project_manifest_only(&target_dir).ok()?;
    package_manifest_version(&manifest)
}

fn resolve_locked_version(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    roots: &LinkRoots<'_>,
    alias: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<String> {
    let key = spec.version.resolved_key(alias)?.without_peer();
    let metadata = lockfile.packages.as_ref()?.get(&key)?;
    if let Some(version) = &metadata.version {
        return Some(version.clone());
    }
    let LockfileResolution::Directory(directory) = &metadata.resolution else { return None };
    resolve_link_version(lockfile_dir, roots, &directory.directory)
}

pub(super) fn package_manifest_version(manifest: &PackageManifest) -> Option<String> {
    manifest_str_field(manifest, "version").map(String::from)
}

fn manifest_str_field<'a>(manifest: &'a PackageManifest, field: &str) -> Option<&'a str> {
    manifest
        .value()
        .get(field)
        .and_then(|val| val.as_str())
}

/// A workspace package an importer reaches through `link:`, whose own
/// `peerDependencies` the importer has to satisfy.
pub(super) struct LinkedPackagePeers<'a> {
    pub(super) manifest: &'a PackageManifest,
    pub(super) alias: &'a str,
    pub(super) linked_version: &'a str,
    pub(super) catalogs: Option<&'a Catalogs>,
    pub(super) issues: &'a mut PeerIssues,
    pub(super) providers: PeerProviders<'a>,
}

/// An importer snapshot and the lockfile its resolved specs index into.
#[derive(Clone, Copy)]
pub(super) struct ImporterProvider<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) lockfile_dir: &'a Path,
    pub(super) snapshot: &'a ProjectSnapshot,
    pub(super) dir: &'a Path,
}

#[derive(Clone, Copy)]
pub(super) struct PeerProviders<'a> {
    pub(super) importer: ImporterProvider<'a>,
    pub(super) linked_importer: Option<ImporterProvider<'a>>,
    pub(super) roots: &'a LinkRoots<'a>,
    pub(super) resolve_peers_from_workspace_root: bool,
    pub(super) project_manifests: &'a ProjectManifests<'a>,
}

pub(super) fn check_linked_package_peers(
    inputs: LinkedPackagePeers<'_>,
) -> Result<(), CatalogResolutionError> {
    let issues = inputs.issues;
    let Some(peer_deps) = inputs.manifest
        .value()
        .get("peerDependencies")
        .and_then(|deps_val| deps_val.as_object())
    else {
        return Ok(());
    };

    let current_parents = vec![ParentPkg {
        name: inputs.alias.to_string(),
        version: inputs.linked_version.to_string(),
    }];

    for (peer_name, peer_range_val) in peer_deps {
        let Some(peer_range) = peer_range_val.as_str() else { continue };
        let peer_range = resolve_peer_range(peer_name, peer_range, inputs.catalogs)?;
        check_one_linked_peer(LinkedPeerCheck {
            parents: &current_parents,
            optional: peer_is_optional(inputs.manifest, peer_name),
            peer_name,
            peer_range: &get_peer_version_range(&peer_range),
            issues,
            providers: inputs.providers,
        });
    }
    Ok(())
}

/// One peer dependency of a linked package, and the two importers that could
/// satisfy it.
struct LinkedPeerCheck<'a> {
    parents: &'a [ParentPkg],
    optional: bool,
    peer_name: &'a str,
    peer_range: &'a str,
    issues: &'a mut PeerIssues,
    providers: PeerProviders<'a>,
}

fn check_one_linked_peer(check: LinkedPeerCheck<'_>) {
    let issues = check.issues;
    let Ok(peer_pkg_name) = check.peer_name.parse::<PkgName>() else { return };

    // The linked package's own project comes second: a peer the depending
    // project provides is the one that ends up resolved.
    let resolved_ref = check.providers.resolve_reference(&peer_pkg_name);
    let Some((spec, provider)) = resolved_ref else {
        record_missing_peer(
            issues,
            check.peer_name,
            check.parents,
            check.optional,
            check.peer_range,
        );
        return;
    };

    let found_version = check.providers.resolved_peer_version(provider, &peer_pkg_name, spec);
    let Some(found_version) = found_version else { return };
    record_bad_peer(
        issues,
        check.peer_name,
        check.parents,
        check.optional,
        check.peer_range,
        found_version,
    );
}

/// An unresolved peer is an issue unless the declaration marks it optional.
pub(super) fn record_missing_peer(
    issues: &mut PeerIssues,
    peer_name: &str,
    parents: &[ParentPkg],
    optional: bool,
    wanted_range: &str,
) {
    if optional {
        return;
    }
    issues.missing
        .entry(peer_name.to_string())
        .or_default()
        .push(MissingPeerIssue {
            parents: parents.to_vec(),
            optional,
            wanted_range: wanted_range.to_string(),
        });
}

/// A resolved peer outside the wanted range is an issue, optional or not.
pub(super) fn record_bad_peer(
    issues: &mut PeerIssues,
    peer_name: &str,
    parents: &[ParentPkg],
    optional: bool,
    wanted_range: &str,
    found_version: String,
) {
    if satisfies(&found_version, wanted_range) {
        return;
    }
    issues.bad
        .entry(peer_name.to_string())
        .or_default()
        .push(BadPeerIssue {
            parents: parents.to_vec(),
            optional,
            wanted_range: wanted_range.to_string(),
            found_version,
            resolved_from: Vec::new(),
        });
}

fn peer_is_optional(manifest: &PackageManifest, peer_name: &str) -> bool {
    manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(|meta_map| meta_map.get(peer_name))
        .and_then(|peer_meta| peer_meta.get("optional"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

impl PeerProviders<'_> {
    /// The version a project dependency spec resolves to, whether it names a
    /// registry version, a tarball, a linked directory or a file dependency.
    fn resolved_peer_version(
        &self,
        provider: ImporterProvider<'_>,
        peer_pkg_name: &PkgName,
        spec: &ResolvedDependencySpec,
    ) -> Option<String> {
        if let Some(link_target) = spec.version.as_link_target() {
            return Some(
                self.link_version(provider.dir, link_target)
                    .unwrap_or_else(|| format!("link:{link_target}")),
            );
        }
        resolve_locked_version(
            provider.lockfile,
            provider.lockfile_dir,
            self.roots,
            peer_pkg_name,
            spec,
        )
        .or_else(|| spec.version.ver_peer().map(extract_peer_version))
        .or_else(|| {
            spec.version
                .as_file_target()
                .map(|file_target| format!("file:{file_target}"))
        })
    }

    /// [`resolve_link_version`], with the manifest taken from
    /// [`ProjectManifests`] when the target is one of them.
    fn link_version(&self, base_dir: &Path, link_target: &str) -> Option<String> {
        let canonical_lockfile_dir = self.roots.canonical_lockfile_dir()?;
        let target_dir = self.roots.canonical_target(&base_dir.join(link_target))?;
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(canonical_lockfile_dir, &target_dir);
        let manifest = linked_manifest(self.project_manifests, &importer_id, &target_dir)?;
        package_manifest_version(&manifest)
    }
}

fn project_dependency<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer.dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| {
            importer.dev_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
        .or_else(|| {
            importer.optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
}

fn resolve_peer_range(
    peer_name: &str,
    peer_range: &str,
    catalogs: Option<&Catalogs>,
) -> Result<String, CatalogResolutionError> {
    let Some(catalogs) = catalogs else { return Ok(peer_range.to_string()) };
    let wanted =
        WantedDependency { alias: peer_name.to_string(), bare_specifier: peer_range.to_string() };
    // A peer range names a version range, never a path, so a `file:` /
    // `link:` entry has nothing to re-anchor.
    match resolve_from_catalog(catalogs, &wanted, CatalogAnchor::AsWritten) {
        CatalogResolutionResult::Found(found) => Ok(found.resolution.specifier),
        CatalogResolutionResult::Unused => Ok(peer_range.to_string()),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => Err(misconfiguration.error),
    }
}

impl PeerProviders<'_> {
    fn resolve_reference(
        &self,
        name: &PkgName,
    ) -> Option<(&ResolvedDependencySpec, ImporterProvider<'_>)> {
        project_dependency(self.importer.snapshot, name)
            .map(|spec| (spec, self.importer))
            .or_else(|| {
                self.linked_importer.and_then(|provider| {
                    project_dependency(provider.snapshot, name).map(|spec| (spec, provider))
                })
            })
            .or_else(|| self.root_reference(name))
    }

    fn root_reference(
        &self,
        name: &PkgName,
    ) -> Option<(&ResolvedDependencySpec, ImporterProvider<'_>)> {
        if !self.resolve_peers_from_workspace_root {
            return None;
        }
        let root_importer = self.importer.lockfile.importers.get(".")?;
        let spec = project_dependency(root_importer, name)?;
        Some((
            spec,
            ImporterProvider {
                lockfile: self.importer.lockfile,
                lockfile_dir: self.roots.lockfile,
                snapshot: root_importer,
                dir: self.roots.lockfile,
            },
        ))
    }
}

#[cfg(test)]
mod tests;
