use super::{
    ProjectManifestsByDir,
    injected_manifest::{
        publishing_project_manifest, target_manifest, workspace_path_matches_target,
    },
};
use pnpm_resolving_deps_resolver::locked_child_admits;
use pnpm_resolving_local_resolver::{local_spec_path, recorded_local_path};
use pnpm_workspace_spec::is_workspace_path_specifier;
use std::path::Path;

/// Where the specs of a local directory dependency resolve.
#[derive(Clone, Copy)]
pub(super) struct SpecDirs<'a> {
    pub(super) workspace_root: &'a Path,
    pub(super) lockfile_dir: &'a Path,
    /// The workspace projects' manifests, consulted before a `workspace:`
    /// target's `package.json` on disk: a host that hands its project
    /// manifests over in memory need not have written one.
    pub(super) manifests_by_dir: &'a ProjectManifestsByDir<'a>,
    pub(super) canonical_paths: &'a pnpm_fs::CanonicalPaths,
    /// How a locked child's registry is decided, for the reuse gate.
    pub(super) registries: &'a pnpm_lockfile::RegistryContext,
}

/// Whether the recorded resolution of a local directory dependency's child
/// still satisfies its spec. `None` when the lockfile cannot tell (see
/// [`npm_or_registry_spec_satisfies`]).
pub(super) fn spec_satisfies_snapshot_dep(
    dirs: &SpecDirs<'_>,
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if let Some(matches) =
        file_or_link_spec_satisfies(dirs.lockfile_dir, local_dep_dir, spec, lockfile_dep)
    {
        return Some(matches);
    }
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        return Some(workspace_spec_satisfies(
            dirs,
            local_dep_dir,
            dep_name,
            spec,
            workspace_spec,
            lockfile_dep,
        ));
    }
    if is_locked_to_other_source(spec, lockfile_dep) {
        return Some(false);
    }
    npm_or_registry_spec_satisfies(dirs.registries, dep_name, spec, lockfile_dep)
}

/// Whether a spec that names one exact source, a tarball URL or a git
/// commit, asks for a source other than the one the child is locked to. The
/// lockfile keys such a child by the URL or commit it resolved.
fn is_locked_to_other_source(spec: &str, lockfile_dep: &pnpm_lockfile::SnapshotDepRef) -> bool {
    let locked_id = match lockfile_dep.ver_peer().map(pnpm_lockfile::PkgVerPeer::version) {
        Some(pnpm_lockfile::VersionPart::NonSemver(raw)) => raw.as_str(),
        _ => "",
    };
    if let Some((committish, path)) =
        pnpm_resolving_git_resolver::git_spec_committish_and_path(spec)
    {
        let (locked_source, locked_path) = match locked_id.rsplit_once("path:") {
            Some((source, path)) => (source.trim_end_matches(['#', '&']), Some(path)),
            None => (locked_id, None),
        };
        // Hosted archive ids end in the commit or in `<sha>.tar.gz` or
        // `<project>-<sha>.tar.gz` after it.
        let locked_commit = locked_source
            .rsplit(['#', '/'])
            .map(|segment| segment.strip_suffix(".tar.gz").unwrap_or(segment))
            .find(|segment| is_full_commit(segment))
            .unwrap_or_default();
        return committish.is_some_and(|commit| {
            is_full_commit(&commit)
                && (path.as_deref() != locked_path || !locked_commit.eq_ignore_ascii_case(&commit))
        });
    }
    pnpm_resolving_tarball_resolver::remote_tarball_id(spec)
        .is_some_and(|tarball_id| tarball_id.ok().as_deref() != Some(locked_id))
}

fn is_full_commit(committish: &str) -> bool {
    committish.len() == 40 && committish.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn file_or_link_spec_satisfies(
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if spec.starts_with("link:") {
        let Some(lockfile_target) = lockfile_dep.as_link_target() else {
            return Some(false);
        };
        return Some(
            local_spec_path(spec, local_dep_dir)
                == recorded_local_path(lockfile_target, lockfile_dir),
        );
    }
    let path = spec.strip_prefix("file:")?;
    if let Some(target) = lockfile_dep.as_link_target() {
        return Some(
            local_spec_path(spec, local_dep_dir) == recorded_local_path(target, lockfile_dir),
        );
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return Some(false);
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => Some(
            local_spec_path(spec, local_dep_dir) == recorded_local_path(recorded, lockfile_dir),
        ),
        pnpm_lockfile::VersionPart::NonSemver(raw) => Some(raw == spec || raw == path),
        _ => Some(false),
    }
}

fn workspace_path_spec_satisfies(
    dirs: &SpecDirs<'_>,
    local_dep_dir: &Path,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let resolved_spec = local_spec_path(spec, local_dep_dir);
    if let Some(target) = lockfile_dep.as_link_target() {
        return workspace_path_matches_target(
            dirs,
            &resolved_spec,
            &recorded_local_path(target, dirs.lockfile_dir),
        );
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return false;
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => workspace_path_matches_target(
            dirs,
            &resolved_spec,
            &recorded_local_path(recorded, dirs.lockfile_dir),
        ),
        pnpm_lockfile::VersionPart::NonSemver(raw) => raw == spec || raw == workspace_spec,
        _ => false,
    }
}

fn workspace_spec_satisfies(
    dirs: &SpecDirs<'_>,
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if is_workspace_path_specifier(spec) {
        workspace_path_spec_satisfies(dirs, local_dep_dir, spec, workspace_spec, lockfile_dep)
    } else {
        workspace_range_spec_satisfies(dirs, dep_name, workspace_spec, lockfile_dep)
    }
}

fn workspace_range_spec_satisfies(
    dirs: &SpecDirs<'_>,
    dep_name: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let (target, range) = parse_workspace_range(workspace_spec);
    let expected_name = target.unwrap_or(dep_name);
    if let Some(link) = lockfile_dep.as_link_target() {
        return linked_target_satisfies(dirs, link, expected_name, range);
    }
    if !snapshot_dep_name_matches(lockfile_dep, dep_name, expected_name) {
        return false;
    }
    let Some(recorded) = lockfile_dep
        .ver_peer()
        .and_then(|ver_peer| match ver_peer.version() {
            pnpm_lockfile::VersionPart::File(recorded) => Some(recorded),
            _ => None,
        })
    else {
        return false;
    };
    linked_target_satisfies(dirs, recorded, expected_name, range)
}

/// The aliased package name and the version range of the post-`workspace:`
/// part of a specifier, with every pick-the-highest token (`*`, `^`, `~`, or
/// nothing) as `*`.
fn parse_workspace_range(workspace_spec: &str) -> (Option<&str>, &str) {
    let (target, range_str) = pnpm_workspace_spec::split_alias_version(workspace_spec);
    let parsed_range_str = match range_str {
        "*" | "^" | "~" | "" => "*",
        other => other,
    };
    (target, parsed_range_str)
}

fn snapshot_dep_name_matches(
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
    dep_name: &str,
    expected_name: &str,
) -> bool {
    match lockfile_dep {
        pnpm_lockfile::SnapshotDepRef::Alias(key) => pkg_name_matches(&key.name, expected_name),
        pnpm_lockfile::SnapshotDepRef::Plain(_) => dep_name == expected_name,
        pnpm_lockfile::SnapshotDepRef::Link(_) => false,
    }
}

pub(super) fn importer_link_satisfies(
    dirs: &SpecDirs<'_>,
    project_dir: &Path,
    dep_name: &str,
    spec: &str,
    link_target: &str,
) -> bool {
    if super::is_declared_local_directory(spec) {
        return true;
    }
    if spec.starts_with("link:") {
        return local_spec_path(spec, project_dir) == recorded_local_path(link_target, project_dir);
    }
    let Some(workspace_spec) = spec.strip_prefix("workspace:") else {
        return true;
    };
    if is_workspace_path_specifier(spec) {
        return workspace_path_matches_target(
            dirs,
            &local_spec_path(spec, project_dir),
            &recorded_local_path(link_target, project_dir),
        );
    }
    let (target, parsed_range_str) = parse_workspace_range(workspace_spec);
    if crate::manifest_agreement::spec_is_distribution_tag(parsed_range_str) {
        return true;
    }

    let expected_name = target.unwrap_or(dep_name);
    let link_path = Path::new(link_target);
    if link_path.is_absolute() {
        return false;
    }
    let target_dir = project_dir.join(link_path);
    is_missing_from_checkout(dirs, &target_dir)
        || target_dir_satisfies(dirs, &target_dir, expected_name, parsed_range_str)
}

/// Whether a plain or `npm:` semver range that the lockfile links to a
/// workspace project still accepts that project's version. Any other spec is
/// left to [`importer_link_satisfies`].
pub(super) fn importer_range_link_satisfies(
    dirs: &SpecDirs<'_>,
    project_dir: &Path,
    dep_name: &str,
    spec: &str,
    link_target: &str,
) -> bool {
    if spec.starts_with("workspace:") {
        return true;
    }
    let (target, range) = parse_npm_spec(spec);
    if range.parse::<node_semver::Range>().is_err() {
        return true;
    }
    let target_dir = project_dir.join(link_target);
    is_missing_from_checkout(dirs, &target_dir)
        || target_dir_satisfies(dirs, &target_dir, target.unwrap_or(dep_name), range)
}

/// An unbuilt `publishConfig.directory` is absent too, but its project is
/// checked out, so it does not count as missing.
fn is_missing_from_checkout(dirs: &SpecDirs<'_>, target_dir: &Path) -> bool {
    let normalized = pnpm_fs::lexical_normalize(target_dir);
    !dirs.manifests_by_dir.contains_key(&normalized)
        && matches!(
            std::fs::symlink_metadata(target_dir),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound,
        )
        && publishing_project_manifest(dirs, &normalized).is_none()
}

fn linked_target_satisfies(
    dirs: &SpecDirs<'_>,
    link: &str,
    expected_name: &str,
    range: &str,
) -> bool {
    let link_path = Path::new(link);
    if link_path.is_absolute() {
        return false;
    }
    target_dir_satisfies(dirs, &dirs.lockfile_dir.join(link_path), expected_name, range)
}

fn target_dir_satisfies(
    dirs: &SpecDirs<'_>,
    target_dir: &Path,
    expected_name: &str,
    range: &str,
) -> bool {
    let Some(pkg_json) = target_manifest(dirs, target_dir) else {
        return false;
    };
    if pkg_json.get("name").and_then(serde_json::Value::as_str) != Some(expected_name) {
        return false;
    }
    let version = pkg_json
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    pnpm_workspace_range_resolver::resolve_workspace_range(range, &[version.to_string()]).is_some()
}

/// The resolver's reuse gate decides a semver range or a named-registry
/// specifier (see [`locked_child_admits`]). It resolves any other specifier
/// again, which a freshness check cannot do, so those are accepted when the
/// recorded package is the one they name.
/// `None` when the spec is not a range the locked version can be checked
/// against, such as a tag or a git or tarball spec.
fn npm_or_registry_spec_satisfies(
    registries: &pnpm_lockfile::RegistryContext,
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if let Ok(alias) = pnpm_lockfile::PkgName::parse(dep_name)
        && let Some(key) = lockfile_dep.resolve(&alias)
        && let Some(admits) = locked_child_admits(&alias, &key, spec, registries)
    {
        return Some(admits);
    }
    let (target_pkg, clean_spec) = parse_npm_spec(spec);
    let expected_name = target_pkg.unwrap_or(dep_name);
    if !npm_alias_matches(dep_name, expected_name, lockfile_dep) {
        return Some(false);
    }
    let range = clean_spec.parse::<node_semver::Range>().ok()?;
    let version = lockfile_dep.ver_peer().and_then(extract_semver)?;
    Some(range.satisfies(version))
}

/// The aliased package name (if any) and version range of a `workspace:`,
/// `npm:`, or bare dependency specifier.
pub(crate) fn spec_target_and_range(spec: &str) -> (Option<&str>, &str) {
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        return parse_workspace_range(workspace_spec);
    }
    parse_npm_spec(spec)
}

fn parse_npm_spec(spec: &str) -> (Option<&str>, &str) {
    let Some(stripped) = spec.strip_prefix("npm:") else {
        return (None, spec);
    };
    if stripped.parse::<node_semver::Range>().is_ok() {
        return (None, stripped);
    }
    match stripped.rfind('@') {
        Some(idx) if idx > 0 => {
            let range = &stripped[idx + 1..];
            (Some(&stripped[..idx]), if range.is_empty() { "*" } else { range })
        }
        _ => (Some(stripped), "*"),
    }
}

fn npm_alias_matches(
    dep_name: &str,
    target: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    match lockfile_dep {
        pnpm_lockfile::SnapshotDepRef::Alias(key) => pkg_name_matches(&key.name, target),
        pnpm_lockfile::SnapshotDepRef::Plain(_) => dep_name == target,
        pnpm_lockfile::SnapshotDepRef::Link(_) => false,
    }
}

fn pkg_name_matches(name: &pnpm_lockfile::PkgName, expected: &str) -> bool {
    if let Some(scope) = &name.scope {
        expected
            .strip_prefix('@')
            .and_then(|scoped| scoped.split_once('/'))
            .is_some_and(|(exp_scope, exp_bare)| exp_scope == scope && exp_bare == name.bare)
    } else {
        name.bare == expected
    }
}

fn extract_semver(ver_peer: &pnpm_lockfile::PkgVerPeer) -> Option<&node_semver::Version> {
    if let Some(version) = ver_peer.version_semver() {
        return Some(version);
    }
    ver_peer.registry_qualified().map(|(_, version)| version)
}

#[cfg(test)]
mod tests;
