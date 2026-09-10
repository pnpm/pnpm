use super::{
    BTreeMap, DirectDep, HashSet, PackageManifestError, Path, ResolvedTree, Version,
    WorkspaceRootDep, io, safe_read_package_json_from_dir, unwrap_package_name,
};

/// `link:` / `file:` and the path form of `workspace:` name a directory
/// relative to the project that declares them, so the root's specifier
/// can't be hoisted verbatim — it would reach a different path from the
/// importer the peer is hoisted into, or nothing. A `workspace:` range is
/// not path-relative: it selects the same workspace package from every
/// importer, so it needs none of this.
pub(super) fn is_project_relative_specifier(spec: &str) -> bool {
    spec.starts_with("link:") || spec.starts_with("file:") || spec.starts_with("workspace:.")
}

/// The name and version of the package a project-relative specifier points
/// at, or `None` when there is none to read. Its version is what stands in
/// for the path in [`build_workspace_root_deps`], so the root keeps the
/// authority over the peer that a registry dependency has and the peer
/// resolves to the same package from every importer. The manifest is read
/// from disk rather than taken from the resolved tree, which a linked
/// dependency reused from the lockfile is absent from — reading it makes a
/// repeat install hoist what a fresh install of the same manifest hoists.
pub(super) fn local_target_identity(
    spec: &str,
    project_dir: &Path,
) -> Result<Option<LocalTargetIdentity>, PackageManifestError> {
    let path_without_protocol = spec.split_once(':').map_or(spec, |(_, path)| path);
    let Some(manifest) = read_manifest_of_local_target(&project_dir.join(path_without_protocol))?
    else {
        return Ok(None);
    };
    let Some(version) = manifest.get("version").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    if version.parse::<Version>().is_err() {
        return Ok(None);
    }
    Ok(Some(LocalTargetIdentity {
        name: manifest.get("name").and_then(serde_json::Value::as_str).map(str::to_string),
        version: version.to_string(),
    }))
}

/// What a project-relative root dep is pinned to. See
/// [`local_target_identity`].
pub(super) struct LocalTargetIdentity {
    /// `None` when the manifest carries no name, leaving the dep under the
    /// name it was already listed by.
    pub(super) name: Option<String>,
    pub(super) version: String,
}

/// [`safe_read_package_json_from_dir`] maps only a missing file to
/// `Ok(None)`; a `file:` target is a tarball as often as a directory, and
/// a path component of a tarball is not a directory to read a manifest
/// from.
pub(super) fn read_manifest_of_local_target(
    dir: &Path,
) -> Result<Option<serde_json::Value>, PackageManifestError> {
    match safe_read_package_json_from_dir(dir) {
        Err(PackageManifestError::Io(err)) if err.kind() == io::ErrorKind::NotADirectory => {
            Ok(None)
        }
        result => result,
    }
}

/// `name_ver`, else the manifest — the canonical name for the protocols
/// that leave `name_ver` unset. The git and remote-tarball resolvers fill
/// `manifest` from the package's own `package.json` during resolution
/// whenever they hold a fetch context, which every install path wires, so
/// those arrive named too. `None` when no manifest name is available: a
/// repository with no `package.json`, or the resolve-only chain that
/// wires no fetch context — and hoists no peers.
pub(super) fn resolved_pkg_name(
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Option<String> {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return Some(name_ver.name.to_string());
    }
    result.manifest.as_deref()?.get("name").and_then(serde_json::Value::as_str).map(str::to_string)
}

/// The picker matches by alias *and* by real package name, so a dep
/// [`resolved_pkg_name`] can't name still enters under its alias —
/// usually the package name anyway, and better than no candidate.
pub(super) fn build_workspace_root_deps(
    direct: &[DirectDep],
    snapshot: &ResolvedTree,
    declared: &BTreeMap<String, String>,
    project_dir: &Path,
) -> Result<Vec<WorkspaceRootDep>, PackageManifestError> {
    let mut out = Vec::with_capacity(direct.len());
    let mut named = HashSet::default();
    for dep in direct {
        let Some(pkg) = snapshot.packages.get(dep.id.as_str()) else { continue };
        let Some(pkg_name) = resolved_pkg_name(&pkg.result) else { continue };
        named.insert(dep.alias.as_str());
        out.push(WorkspaceRootDep {
            alias: dep.alias.clone(),
            pkg_name,
            normalized_bare_specifier: pkg
                .result
                .normalized_bare_specifier
                .clone()
                .or_else(|| declared.get(&dep.alias).cloned()),
        });
    }
    for (alias, bare_specifier) in declared {
        if named.contains(alias.as_str()) {
            continue;
        }
        let (pkg_name, _) = unwrap_package_name(alias, bare_specifier);
        out.push(WorkspaceRootDep {
            alias: alias.clone(),
            pkg_name: pkg_name.to_string(),
            normalized_bare_specifier: Some(bare_specifier.clone()),
        });
    }
    for dep in &mut out {
        apply_local_target_identity(dep, project_dir)?;
    }
    Ok(out)
}

/// Replace a root dependency declared with a local protocol by the identity
/// its target's manifest names.
pub(super) fn apply_local_target_identity(
    dep: &mut WorkspaceRootDep,
    project_dir: &Path,
) -> Result<(), PackageManifestError> {
    // Cloned so the identity can be written back onto `dep`; only the
    // handful of root deps declared with a local protocol reach here.
    let Some(spec) = dep.normalized_bare_specifier.clone() else { return Ok(()) };
    if !is_project_relative_specifier(&spec) {
        return Ok(());
    }
    // A path is never hoistable, so a target that names no version to
    // stand in for it leaves the root offering no candidate at all.
    dep.normalized_bare_specifier = None;
    let Some(identity) = local_target_identity(&spec, project_dir)? else { return Ok(()) };
    if let Some(name) = identity.name {
        dep.pkg_name = name;
    }
    dep.normalized_bare_specifier = Some(identity.version);
    Ok(())
}
