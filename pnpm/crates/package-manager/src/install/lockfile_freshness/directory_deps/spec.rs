use std::path::Path;

pub(super) fn spec_satisfies_snapshot_dep(
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

fn file_or_link_spec_satisfies(
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if let Some(target) = spec.strip_prefix("link:") {
        let lockfile_target = lockfile_dep.as_link_target()?;
        return Some(
            pnpm_fs::lexical_normalize(&local_dep_dir.join(target))
                == pnpm_fs::lexical_normalize(&lockfile_dir.join(lockfile_target)),
        );
    }
    let path = spec.strip_prefix("file:")?;
    if let Some(target) = lockfile_dep.as_link_target() {
        return Some(
            pnpm_fs::lexical_normalize(&local_dep_dir.join(path))
                == pnpm_fs::lexical_normalize(&lockfile_dir.join(target)),
        );
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return Some(false);
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => Some(
            pnpm_fs::lexical_normalize(&local_dep_dir.join(path))
                == pnpm_fs::lexical_normalize(&lockfile_dir.join(recorded)),
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
    let clean_spec = workspace_spec.strip_prefix("./").unwrap_or(workspace_spec);
    let resolved_spec = pnpm_fs::lexical_normalize(&local_dep_dir.join(clean_spec));
    if let Some(target) = lockfile_dep.as_link_target() {
        return pnpm_fs::lexical_normalize(&lockfile_dir.join(target)) == resolved_spec;
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return false;
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => {
            pnpm_fs::lexical_normalize(&lockfile_dir.join(recorded)) == resolved_spec
        }
        pnpm_lockfile::VersionPart::NonSemver(raw) => raw == spec || raw == workspace_spec,
        _ => false,
    }
}

fn workspace_spec_satisfies(
    lockfile_dir: &Path,
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if workspace_spec.starts_with('.') || workspace_spec.starts_with('/') {
        return workspace_path_spec_satisfies(
            lockfile_dir,
            local_dep_dir,
            spec,
            workspace_spec,
            lockfile_dep,
        );
    }
    let (target, parsed_range_str) = parse_workspace_range(workspace_spec);
    let expected_name = target.unwrap_or(dep_name);
    let Ok(range) = parsed_range_str.parse::<node_semver::Range>() else {
        return false;
    };
    if let Some(link) = lockfile_dep.as_link_target() {
        return linked_target_satisfies(
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
    if let Some(version) = lockfile_dep.ver_peer().and_then(extract_semver) {
        return range.satisfies(version);
    }
    lockfile_dep
        .ver_peer()
        .is_some_and(|ver_peer| matches!(ver_peer.version(), pnpm_lockfile::VersionPart::File(_)))
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
    if !pnpm_fs::is_subdir(lockfile_dir, &target_dir) {
        return false;
    }
    let Ok(Some(pkg_json)) = pnpm_package_manifest::safe_read_package_json_from_dir(&target_dir)
    else {
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

fn npm_or_registry_spec_satisfies(
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let (target_pkg, clean_spec) = parse_npm_spec(spec);
    if let Some(target) = target_pkg
        && !npm_alias_matches(dep_name, target, lockfile_dep)
    {
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
