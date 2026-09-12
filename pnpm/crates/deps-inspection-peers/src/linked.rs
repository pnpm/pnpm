use super::{
    BadPeerIssue, CatalogResolutionError, CatalogResolutionResult, Catalogs, Lockfile,
    LockfileResolution, MissingPeerIssue, PackageManifest, ParentPkg, Path, PathBuf, PeerIssues,
    PkgName, ProjectSnapshot, ResolvedDependencySpec, WantedDependency, get_peer_version_range,
    resolve_from_catalog, satisfies,
};

pub(super) struct CanonicalPathWithin {
    pub(super) path: PathBuf,
    pub(super) base: PathBuf,
}

pub(super) fn canonical_path_within(path: &Path, base: &Path) -> Option<CanonicalPathWithin> {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return None;
    };
    canonical_path
        .starts_with(&canonical_base)
        .then_some(CanonicalPathWithin { path: canonical_path, base: canonical_base })
}

/// `base_dir` is the directory the `link:` target is relative to — the
/// importer's directory for importer dependencies, the lockfile directory for
/// snapshot dependencies. Targets escaping `lockfile_dir` are rejected.
pub(super) fn resolve_link_version(
    base_dir: &Path,
    lockfile_dir: &Path,
    link_target: &str,
) -> Option<String> {
    let target_dir = canonical_path_within(&base_dir.join(link_target), lockfile_dir)?.path;
    let manifest = PackageManifest::from_path(target_dir.join("package.json")).ok()?;
    package_manifest_version(&manifest)
}

fn resolve_file_version(
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
    manifest.value().get("version").and_then(|version| version.as_str()).map(String::from)
}

/// A workspace package an importer reaches through `link:`, whose own
/// `peerDependencies` the importer has to satisfy.
pub(super) struct LinkedPackagePeers<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) importer: &'a ProjectSnapshot,
    pub(super) linked_importer: Option<&'a ProjectSnapshot>,
    pub(super) importer_dir: &'a Path,
    pub(super) linked_importer_dir: &'a Path,
    pub(super) lockfile_dir: &'a Path,
    pub(super) manifest: &'a PackageManifest,
    pub(super) alias: &'a str,
    pub(super) linked_version: &'a str,
    pub(super) catalogs: Option<&'a Catalogs>,
    pub(super) issues: &'a mut PeerIssues,
}

pub(super) fn check_linked_package_peers(
    inputs: LinkedPackagePeers<'_>,
) -> Result<(), CatalogResolutionError> {
    let issues = inputs.issues;
    let Some(peer_deps) =
        inputs.manifest.value().get("peerDependencies").and_then(|deps_val| deps_val.as_object())
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
            lockfile: inputs.lockfile,
            importer: inputs.importer,
            linked_importer: inputs.linked_importer,
            importer_dir: inputs.importer_dir,
            linked_importer_dir: inputs.linked_importer_dir,
            lockfile_dir: inputs.lockfile_dir,
            parents: &current_parents,
            optional: peer_is_optional(inputs.manifest, peer_name),
            peer_name,
            peer_range: &get_peer_version_range(&peer_range),
            issues,
        });
    }
    Ok(())
}

/// One peer dependency of a linked package, and the two importers that could
/// satisfy it.
struct LinkedPeerCheck<'a> {
    lockfile: &'a Lockfile,
    importer: &'a ProjectSnapshot,
    linked_importer: Option<&'a ProjectSnapshot>,
    importer_dir: &'a Path,
    linked_importer_dir: &'a Path,
    lockfile_dir: &'a Path,
    parents: &'a [ParentPkg],
    optional: bool,
    peer_name: &'a str,
    peer_range: &'a str,
    issues: &'a mut PeerIssues,
}

fn check_one_linked_peer(check: LinkedPeerCheck<'_>) {
    let issues = check.issues;
    let Ok(peer_pkg_name) = check.peer_name.parse::<PkgName>() else { return };

    // The linked package's own project comes second: a peer the depending
    // project provides is the one that ends up resolved.
    let resolved_ref = project_dependency(check.importer, &peer_pkg_name)
        .map(|spec| (spec, check.importer_dir))
        .or_else(|| {
            check
                .linked_importer
                .and_then(|importer| project_dependency(importer, &peer_pkg_name))
                .map(|spec| (spec, check.linked_importer_dir))
        });
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

    let found_version = resolved_peer_version(
        check.lockfile,
        check.lockfile_dir,
        dependency_dir,
        &peer_pkg_name,
        spec,
    );
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
    issues.missing.entry(peer_name.to_string()).or_default().push(MissingPeerIssue {
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
    issues.bad.entry(peer_name.to_string()).or_default().push(BadPeerIssue {
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

/// The version a project dependency spec resolves to, whether it names a
/// registry version, a linked directory or a file dependency.
fn resolved_peer_version(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    dependency_dir: &Path,
    peer_pkg_name: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<String> {
    if let Some(ver_peer) = spec.version.ver_peer() {
        return Some(ver_peer.version().to_string());
    }
    if let Some(link_target) = spec.version.as_link_target() {
        return Some(
            resolve_link_version(dependency_dir, lockfile_dir, link_target)
                .unwrap_or_else(|| format!("link:{link_target}")),
        );
    }
    spec.version.as_file_target().map(|file_target| {
        resolve_file_version(lockfile, lockfile_dir, peer_pkg_name, spec)
            .unwrap_or_else(|| format!("file:{file_target}"))
    })
}

fn project_dependency<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| importer.dev_dependencies.as_ref().and_then(|deps| deps.get(name)))
        .or_else(|| importer.optional_dependencies.as_ref().and_then(|deps| deps.get(name)))
}

fn resolve_peer_range(
    peer_name: &str,
    peer_range: &str,
    catalogs: Option<&Catalogs>,
) -> Result<String, CatalogResolutionError> {
    let Some(catalogs) = catalogs else { return Ok(peer_range.to_string()) };
    let wanted =
        WantedDependency { alias: peer_name.to_string(), bare_specifier: peer_range.to_string() };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok(found.resolution.specifier),
        CatalogResolutionResult::Unused => Ok(peer_range.to_string()),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => Err(misconfiguration.error),
    }
}
