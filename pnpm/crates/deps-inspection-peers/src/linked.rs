use super::{
    BadPeerIssue, CatalogAnchor, CatalogResolutionError, CatalogResolutionResult, Catalogs,
    LinkedDependency, Lockfile, LockfileResolution, MissingPeerIssue, PackageManifest, ParentPkg,
    Path, PathBuf, PeerIssues, PkgName, ProjectManifests, ProjectSnapshot, ResolvedDependencySpec,
    WantedDependency, extract_peer_version, get_peer_version_range, resolve_from_catalog,
    satisfies,
};
use std::{borrow::Cow, collections::HashMap, path::Component};

pub(super) fn canonical_path_within(path: &Path, base: &Path) -> Option<PathBuf> {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return None;
    };
    canonical_path.starts_with(&canonical_base).then_some(canonical_path)
}

/// `base_dir` is the directory the `link:` target is relative to — the
/// importer's directory for importer dependencies, the lockfile directory for
/// snapshot dependencies. Targets escaping `lockfile_dir` are rejected.
pub(super) fn resolve_link_version(
    base_dir: &Path,
    lockfile_dir: &Path,
    link_target: &str,
) -> Option<String> {
    let target_dir = canonical_path_within(&base_dir.join(link_target), lockfile_dir)?;
    let manifest = pnpm_workspace::read_project_manifest_only(&target_dir).ok()?;
    package_manifest_version(&manifest)
}

fn resolve_locked_version(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    alias: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<String> {
    let key = spec.version.resolved_key(alias)?.without_peer();
    let metadata = lockfile.packages.as_ref()?.get(&key)?;
    if let Some(version) = &metadata.version {
        return Some(version.clone());
    }
    let LockfileResolution::Directory(directory) = &metadata.resolution else { return None };
    resolve_link_version(lockfile_dir, lockfile_dir, &directory.directory)
}

pub(super) fn package_manifest_version(manifest: &PackageManifest) -> Option<String> {
    manifest
        .value()
        .get("version")
        .and_then(|version| version.as_str())
        .map(String::from)
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

#[derive(Clone, Copy)]
pub(super) struct PeerProviders<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) importer: &'a ProjectSnapshot,
    pub(super) linked_importer: Option<&'a ProjectSnapshot>,
    pub(super) importer_dir: &'a Path,
    pub(super) linked_importer_dir: &'a Path,
    pub(super) lockfile_dir: &'a Path,
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
    let Some((spec, dependency_dir)) = resolved_ref else {
        record_missing_peer(
            issues,
            check.peer_name,
            check.parents,
            check.optional,
            check.peer_range,
        );
        return;
    };

    let found_version = check.providers.resolved_peer_version(dependency_dir, &peer_pkg_name, spec);
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
        dependency_dir: &Path,
        peer_pkg_name: &PkgName,
        spec: &ResolvedDependencySpec,
    ) -> Option<String> {
        if let Some(link_target) = spec.version.as_link_target() {
            return Some(
                self.link_version(dependency_dir, link_target)
                    .unwrap_or_else(|| format!("link:{link_target}")),
            );
        }
        resolve_locked_version(self.lockfile, self.lockfile_dir, peer_pkg_name, spec)
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
        let canonical_lockfile_dir = dunce::canonicalize(self.lockfile_dir).ok()?;
        let target_dir = dunce::canonicalize(base_dir.join(link_target))
            .ok()
            .filter(|target_dir| target_dir.starts_with(&canonical_lockfile_dir))?;
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(&canonical_lockfile_dir, &target_dir);
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
    fn resolve_reference(&self, name: &PkgName) -> Option<(&ResolvedDependencySpec, &Path)> {
        project_dependency(self.importer, name)
            .map(|spec| (spec, self.importer_dir))
            .or_else(|| {
                self.linked_importer
                    .and_then(|importer| project_dependency(importer, name))
                    .map(|spec| (spec, self.linked_importer_dir))
            })
            .or_else(|| self.root_reference(name))
    }

    fn root_reference(&self, name: &PkgName) -> Option<(&ResolvedDependencySpec, &Path)> {
        if !self.resolve_peers_from_workspace_root {
            return None;
        }
        self.lockfile.importers
            .get(".")
            .and_then(|root_importer| project_dependency(root_importer, name))
            .map(|spec| (spec, self.lockfile_dir))
    }
}

/// The linked dependencies one walk has read. Most importers of a workspace
/// link the same packages, so each target is canonicalized and its manifest
/// read once per walk rather than once per importer that links it.
pub(super) struct LinkedPackages<'m> {
    project_manifests: &'m ProjectManifests<'m>,
    canonical_lockfile_dir: Option<PathBuf>,
    key_base_by_importer_dir: HashMap<PathBuf, Option<PathBuf>>,
    pub(super) by_target: HashMap<PathBuf, Option<LinkedDependency<'m>>>,
}

impl<'m> LinkedPackages<'m> {
    pub(super) fn new(lockfile_dir: &Path, project_manifests: &'m ProjectManifests<'m>) -> Self {
        LinkedPackages {
            project_manifests,
            canonical_lockfile_dir: dunce::canonicalize(lockfile_dir).ok(),
            key_base_by_importer_dir: HashMap::new(),
            by_target: HashMap::new(),
        }
    }

    /// The version, the peer check, and the linked importer's own snapshot
    /// all need the same answers. Targets escaping the lockfile directory
    /// are rejected.
    pub(super) fn resolve(
        &mut self,
        importer_dir: &Path,
        link_target: &str,
    ) -> Option<&LinkedDependency<'m>> {
        let target = self.target_key(importer_dir, Path::new(link_target));
        let canonical_lockfile_dir = self.canonical_lockfile_dir.as_deref()?;
        let project_manifests = self.project_manifests;
        self.by_target
            .entry(target)
            .or_insert_with_key(|target| {
                let dir = dunce::canonicalize(target).ok()?;
                if !dir.starts_with(canonical_lockfile_dir) {
                    return None;
                }
                let importer_id =
                    pnpm_workspace::importer_id_from_root_dir(canonical_lockfile_dir, &dir);
                let manifest = linked_manifest(project_manifests, &importer_id, &dir);
                let version = manifest
                    .as_deref()
                    .and_then(package_manifest_version)
                    .unwrap_or_else(|| "0.0.0".to_string());
                Some(LinkedDependency { dir, importer_id, manifest, version })
            })
            .as_ref()
    }

    /// `..` is popped off the importer directory, so every importer
    /// reaching one package shares its entry:
    /// - Unix resolves `..` physically, so it is popped off the canonical
    ///   importer directory.
    /// - Windows resolves `..` lexically on the spelled path, so it is popped
    ///   only when that path is already canonical.
    ///
    /// Any other importer keeps the path it spells.
    fn target_key(&mut self, importer_dir: &Path, link_target: &Path) -> PathBuf {
        let base = self.key_base_by_importer_dir
            .entry(importer_dir.to_path_buf())
            .or_insert_with_key(|dir| {
                let canonical = dunce::canonicalize(dir).ok()?;
                (cfg!(unix) || canonical == *dir).then_some(canonical)
            });
        let Some(base) = base else {
            return importer_dir.join(link_target);
        };
        let mut target = base.clone();
        let mut components = link_target.components().peekable();
        while components.next_if_eq(&Component::ParentDir).is_some() {
            target.pop();
        }
        target.extend(components);
        target
    }
}

/// The manifest of the linked directory `dir`, taken from
/// [`ProjectManifests`] when `importer_id` names a loaded project.
fn linked_manifest<'m>(
    project_manifests: &ProjectManifests<'m>,
    importer_id: &str,
    dir: &Path,
) -> Option<Cow<'m, PackageManifest>> {
    match project_manifests.get(importer_id) {
        Some(&manifest) => Some(Cow::Borrowed(manifest)),
        None => pnpm_workspace::read_project_manifest_only(dir).ok().map(Cow::Owned),
    }
}

#[cfg(test)]
mod tests;
