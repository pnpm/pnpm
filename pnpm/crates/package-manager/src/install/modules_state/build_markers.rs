use super::super::{Config, HashSet, Lockfile, Path};

/// Whether a GVS install can own slots whose interrupted build or patch
/// application must be recovered from `.pnpm-needs-build`.
///
/// The marker is shared store state, so neither optimistic workspace state nor
/// the frozen importer's symlinks can prove it absent. Only configurations
/// capable of acting on one need to leave those no-op paths.
pub(in super::super) fn gvs_build_markers_may_require_recovery(config: &Config) -> bool {
    config.enable_global_virtual_store
        && (config.dangerously_allow_all_builds
            || config.allow_builds.values().any(|allowed| *allowed)
            || config.patched_dependencies.as_ref().is_some_and(|patches| !patches.is_empty()))
}
/// Probe the buildable or patched GVS slots this lockfile resolves to.
/// Markers in sibling hash directories belong to other dependency graphs and
/// cannot be recovered by materializing this one. The effective Node version
/// participates only when materialization would run installability checks;
/// constraint-free materialization keys the layout to the detected host Node.
pub(in super::super) fn gvs_build_marker_present(
    wanted: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    effective_node_version: Option<&str>,
) -> bool {
    if !gvs_build_markers_may_require_recovery(config) {
        return false;
    }
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    let Some(snapshots) = wanted.snapshots.as_ref() else {
        return false;
    };
    let eligible_snapshots = snapshots
        .keys()
        .filter(|snapshot_key| {
            crate::snapshot_has_patch(snapshot_key)
                || policy.check(&snapshot_key.without_peer().to_string()) == Some(true)
        })
        .collect::<Vec<_>>();
    match sibling_store_marker(wanted, config, &eligible_snapshots) {
        MarkerProbe::Unreadable => return true,
        MarkerProbe::None => return false,
        MarkerProbe::Found => {}
    }
    let layout = crate::virtual_store_layout_for_lockfile(
        config,
        installability_node_version(wanted, config, effective_node_version),
        wanted.snapshots.as_ref(),
        wanted.packages.as_ref(),
        Some(&policy),
        Some(lockfile_dir),
    );
    if crate::validate_virtual_store_slot_containment(wanted.snapshots.as_ref(), &layout).is_err() {
        return true;
    }
    any_slot_build_marker(&eligible_snapshots, &layout)
}
/// What a `.pnpm-needs-build` probe found.
pub(super) enum MarkerProbe {
    None,
    Found,
    /// The probe could not complete, so recovery has to be assumed.
    Unreadable,
}
/// Probe every version directory the eligible snapshots resolve to, including
/// the sibling hash directories that belong to other dependency graphs.
pub(super) fn sibling_store_marker(
    wanted: &Lockfile,
    config: &Config,
    eligible_snapshots: &[&pnpm_lockfile::PackageKey],
) -> MarkerProbe {
    let mut visited_version_dirs = HashSet::new();
    for &snapshot_key in eligible_snapshots {
        let metadata = wanted
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&snapshot_key.without_peer()));
        let Some(version_dir) = crate::global_virtual_store_version_dir(
            &config.global_virtual_store_dir,
            snapshot_key,
            metadata,
        ) else {
            return MarkerProbe::Unreadable;
        };
        if !visited_version_dirs.insert(version_dir.clone()) {
            continue;
        }
        match version_dir_marker(&version_dir, snapshot_key) {
            MarkerProbe::None => {}
            found => return found,
        }
    }
    MarkerProbe::None
}
pub(super) fn version_dir_marker(
    version_dir: &Path,
    snapshot_key: &pnpm_lockfile::PackageKey,
) -> MarkerProbe {
    let hash_dirs = match std::fs::read_dir(version_dir) {
        Ok(hash_dirs) => hash_dirs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return MarkerProbe::None,
        Err(_) => return MarkerProbe::Unreadable,
    };
    for hash_dir in hash_dirs {
        match hash_dir_marker(hash_dir, snapshot_key) {
            MarkerProbe::None => {}
            found => return found,
        }
    }
    MarkerProbe::None
}
pub(super) fn hash_dir_marker(
    hash_dir: std::io::Result<std::fs::DirEntry>,
    snapshot_key: &pnpm_lockfile::PackageKey,
) -> MarkerProbe {
    let Ok(hash_dir) = hash_dir else {
        return MarkerProbe::Unreadable;
    };
    let Ok(file_type) = hash_dir.file_type() else {
        return MarkerProbe::Unreadable;
    };
    if !file_type.is_dir() {
        return MarkerProbe::None;
    }
    let Ok(pkg_dir) = crate::safe_join_modules_dir::safe_join_modules_dir(
        &hash_dir.path().join("node_modules"),
        &snapshot_key.name.to_string(),
    ) else {
        return MarkerProbe::Unreadable;
    };
    if pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file() {
        return MarkerProbe::Found;
    }
    MarkerProbe::None
}
/// The effective Node version participates only when materialization would
/// run installability checks; constraint-free materialization keys the layout
/// to the detected host Node.
pub(super) fn installability_node_version<'a>(
    wanted: &Lockfile,
    config: &Config,
    effective_node_version: Option<&'a str>,
) -> Option<&'a str> {
    let (Some(snapshots), Some(packages)) = (&wanted.snapshots, &wanted.packages) else {
        return None;
    };
    (!config.force
        && !snapshots.is_empty()
        && crate::any_installability_constraint(snapshots, packages))
    .then_some(effective_node_version)
    .flatten()
}
/// Probe the slots this lockfile's own layout resolves to.
pub(super) fn any_slot_build_marker(
    eligible_snapshots: &[&pnpm_lockfile::PackageKey],
    layout: &crate::VirtualStoreLayout,
) -> bool {
    for snapshot_key in eligible_snapshots {
        let Ok(pkg_dir) = crate::safe_join_modules_dir::safe_join_modules_dir(
            &layout.slot_dir(snapshot_key).join("node_modules"),
            &snapshot_key.name.to_string(),
        ) else {
            return true;
        };
        if pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file() {
            return true;
        }
    }
    false
}
