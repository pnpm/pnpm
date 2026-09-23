use std::path::Path;

pub(super) fn spec_satisfies_snapshot_dep(
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if let Some(matches) = file_or_link_spec_satisfies(spec, lockfile_dep) {
        return matches;
    }
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        return workspace_spec_satisfies(
            local_dep_dir,
            dep_name,
            spec,
            workspace_spec,
            lockfile_dep,
        );
    }
    let clean_spec = if let Some(stripped) = spec.strip_prefix("npm:") {
        stripped
            .rfind('@')
            .map_or(stripped, |idx| &stripped[idx + 1..])
    } else {
        spec
    };
    let Ok(range) = clean_spec.parse::<node_semver::Range>() else {
        return true;
    };
    let Some(version) = lockfile_dep.ver_peer().and_then(|ver_peer| ver_peer.version_semver())
    else {
        return true;
    };
    range.satisfies(version)
}

fn file_or_link_spec_satisfies(
    spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> Option<bool> {
    if let Some(target) = spec.strip_prefix("link:") {
        return Some(lockfile_dep.as_link_target() == Some(target));
    }
    let path = spec.strip_prefix("file:")?;
    if let Some(target) = lockfile_dep.as_link_target() {
        return Some(target == path);
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return Some(false);
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => Some(recorded == path),
        pnpm_lockfile::VersionPart::NonSemver(raw) => Some(raw == spec || raw == path),
        _ => Some(false),
    }
}

fn workspace_path_spec_satisfies(
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    let clean_spec = workspace_spec.strip_prefix("./").unwrap_or(workspace_spec);
    if let Some(target) = lockfile_dep.as_link_target() {
        return target.strip_prefix("./").unwrap_or(target) == clean_spec;
    }
    let Some(ver_peer) = lockfile_dep.ver_peer() else {
        return false;
    };
    match ver_peer.version() {
        pnpm_lockfile::VersionPart::File(recorded) => {
            recorded.strip_prefix("./").unwrap_or(recorded) == clean_spec
        }
        pnpm_lockfile::VersionPart::NonSemver(raw) => raw == spec || raw == workspace_spec,
        _ => false,
    }
}

fn workspace_spec_satisfies(
    local_dep_dir: &Path,
    dep_name: &str,
    spec: &str,
    workspace_spec: &str,
    lockfile_dep: &pnpm_lockfile::SnapshotDepRef,
) -> bool {
    if workspace_spec.starts_with('.') || workspace_spec.starts_with('/') {
        return workspace_path_spec_satisfies(spec, workspace_spec, lockfile_dep);
    }
    let (target, range_str) = match workspace_spec.rfind('@') {
        Some(idx) if idx > 0 => (Some(&workspace_spec[..idx]), &workspace_spec[idx + 1..]),
        _ => (None, workspace_spec),
    };
    let expected_name = target.unwrap_or(dep_name);
    let parsed_range_str = match range_str {
        "*" | "^" | "~" | "" => "*",
        other => other,
    };
    let Ok(range) = parsed_range_str.parse::<node_semver::Range>() else {
        return false;
    };
    if let Some(link) = lockfile_dep.as_link_target() {
        return linked_target_satisfies(local_dep_dir, link, expected_name, range_str, &range);
    }
    if let Some(version) = lockfile_dep.ver_peer().and_then(|ver_peer| ver_peer.version_semver()) {
        return range.satisfies(version);
    }
    lockfile_dep
        .ver_peer()
        .is_some_and(|ver_peer| matches!(ver_peer.version(), pnpm_lockfile::VersionPart::File(_)))
}

fn linked_target_satisfies(
    local_dep_dir: &Path,
    link: &str,
    expected_name: &str,
    range_str: &str,
    range: &node_semver::Range,
) -> bool {
    let target_dir = local_dep_dir.join(link);
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
    let Some(ver) = pkg_json
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(|raw_version| raw_version.parse::<node_semver::Version>().ok())
    else {
        return false;
    };
    range.satisfies(&ver)
}
