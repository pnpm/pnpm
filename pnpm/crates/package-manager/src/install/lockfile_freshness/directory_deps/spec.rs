use std::path::{Path, PathBuf};

pub(super) fn spec_satisfies_snapshot_dep(
    workspace_root: &Path,
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if let Some(matches) =
        file_or_link_spec_satisfies(lockfile_dir, local_dep_dir, spec, lockfile_dep)
    {
        return matches;
    }
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        return workspace_spec_satisfies(
            workspace_root,
            lockfile_dir,
            local_dep_dir,
            dep_name,
            spec,
            workspace_spec,
            lockfile_dep,
        );
    }
    npm_or_registry_spec_satisfies(dep_name, spec, lockfile_dep)
}

fn resolve_local_spec_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let clean = raw_path.strip_prefix("./").unwrap_or(raw_path);
    if let Some(rest) = clean
        .strip_prefix("~/")
        .or_else(|| clean.strip_prefix(r"~\"))
    {
        let home = home::home_dir().unwrap_or_default();
        pnpm_fs::lexical_normalize(&home.join(rest))
    } else {
        pnpm_fs::lexical_normalize(&base_dir.join(clean))
    }
}

fn file_or_link_spec_satisfies(
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if let Some(target) = spec.strip_prefix("link:") {
        let Some(lockfile_target) = lockfile_dep.as_link_target() else {
            return Some(false);
        };
        return Some(
            resolve_local_spec_path(local_dep_dir, target)
                == resolve_local_spec_path(lockfile_dir, lockfile_target),
        );
    }
    let path = spec.strip_prefix("file:")?;
    if let Some(target) = lockfile_dep.as_link_target() {
        return Some(
            resolve_local_spec_path(local_dep_dir, path)
                == resolve_local_spec_path(lockfile_dir, target),
        );
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return Some(false);
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => Some(
            resolve_local_spec_path(local_dep_dir, path)
                == resolve_local_spec_path(lockfile_dir, recorded),
        ),
        pnpm_lockfile::VersionPart::NonSemver(raw) => Some(raw == spec || raw == path),
        _ => Some(false),
    }
}

fn workspace_path_spec_satisfies(
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let resolved_spec = resolve_local_spec_path(local_dep_dir, workspace_spec);
    if let Some(target) = lockfile_dep.as_link_target() {
        return resolve_local_spec_path(lockfile_dir, target) == resolved_spec;
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return false;
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => {
            resolve_local_spec_path(lockfile_dir, recorded) == resolved_spec
        }
        pnpm_lockfile::VersionPart::NonSemver(raw) => raw == spec || raw == workspace_spec,
        _ => false,
    }
}

fn is_workspace_path(workspace_spec: &str) -> bool {
    pnpm_local_spec::is_filespec(workspace_spec)
}

fn workspace_spec_satisfies(
    workspace_root: &Path,
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if is_workspace_path(workspace_spec) {
        workspace_path_spec_satisfies(
            lockfile_dir,
            local_dep_dir,
            spec,
            workspace_spec,
            lockfile_dep,
        )
    } else {
        workspace_range_spec_satisfies(
            workspace_root,
            lockfile_dir,
            dep_name,
            workspace_spec,
            lockfile_dep,
        )
    }
}

fn workspace_range_spec_satisfies(
    workspace_root: &Path,
    lockfile_dir: &Path,
    dep_name: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let (target, parsed_range_str) = parse_workspace_range(workspace_spec);
    let expected_name = target.unwrap_or(dep_name);
    let Ok(range) = parsed_range_str.parse::<node_semver::Range>() else {
        return false;
    };
    if let Some(link) = lockfile_dep.as_link_target() {
        return linked_target_satisfies(
            workspace_root,
            lockfile_dir,
            link,
            expected_name,
            parsed_range_str,
            &range,
        );
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
    linked_target_satisfies(
        workspace_root,
        lockfile_dir,
        recorded,
        expected_name,
        parsed_range_str,
        &range,
    )
}

fn parse_workspace_range(workspace_spec: &str) -> (Option<&str>, &str) {
    let (target, range_str) = match workspace_spec.rfind('@') {
        Some(idx) if idx > 0 => (Some(&workspace_spec[..idx]), &workspace_spec[idx + 1..]),
        _ => (None, workspace_spec),
    };
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

fn linked_target_satisfies(
    workspace_root: &Path,
    lockfile_dir: &Path,
    link: &str,
    expected_name: &str,
    range_str: &str,
    range: &node_semver::Range,
) -> bool {
    let link_path = Path::new(link);
    if link_path.is_absolute() {
        return false;
    }
    let target_dir = lockfile_dir.join(link_path);
    let Ok(canonical_manifest) = target_manifest_within_workspace(workspace_root, &target_dir)
    else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(&canonical_manifest) else {
        return false;
    };
    let Ok(pkg_json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    if pkg_json.get("name").and_then(serde_json::Value::as_str) != Some(expected_name) {
        return false;
    }
    if matches!(range_str, "*" | "^" | "~" | "") {
        return true;
    }
    let Some(version) = pkg_json
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(|raw_version| raw_version.parse::<node_semver::Version>().ok())
    else {
        return false;
    };
    range.satisfies(&version)
}

fn target_manifest_within_workspace(
    workspace_root: &Path,
    target_dir: &Path,
) -> Result<PathBuf, ()> {
    if !pnpm_fs::is_subdir(workspace_root, target_dir) {
        return Err(());
    }
    let canonical_root = std::fs::canonicalize(workspace_root).map_err(|_| ())?;
    let canonical_target = std::fs::canonicalize(target_dir).map_err(|_| ())?;
    if !pnpm_fs::is_subdir(&canonical_root, &canonical_target) {
        return Err(());
    }
    let canonical_manifest =
        std::fs::canonicalize(target_dir.join("package.json")).map_err(|_| ())?;
    if !pnpm_fs::is_subdir(&canonical_root, &canonical_manifest) {
        return Err(());
    }
    Ok(canonical_manifest)
}

fn npm_or_registry_spec_satisfies(
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let (target_pkg, clean_spec) = parse_npm_spec(spec);
    let expected_name = target_pkg.unwrap_or(dep_name);
    if !npm_alias_matches(dep_name, expected_name, lockfile_dep) {
        return false;
    }
    let Ok(range) = clean_spec.parse::<node_semver::Range>() else {
        return true;
    };
    let Some(version) = lockfile_dep.ver_peer().and_then(extract_semver) else {
        return true;
    };
    range.satisfies(version)
}

fn parse_npm_spec(spec: &str) -> (Option<&str>, &str) {
    let Some(stripped) = spec.strip_prefix("npm:") else {
        return (None, spec);
    };
    if stripped.parse::<node_semver::Range>().is_ok() {
        return (None, stripped);
    }
    match stripped.rfind('@') {
        Some(idx) if idx > 0 => (Some(&stripped[..idx]), &stripped[idx + 1..]),
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
