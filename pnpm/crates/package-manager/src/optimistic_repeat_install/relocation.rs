//! A `node_modules` that moved or was copied together with its project:
//! recognizing the move from the workspace state it carries, and proving the
//! tree still works where it is now.

use super::{
    Decision, Host, Lockfile, ManifestDrift, OptimisticRepeatInstallCheck, PackageManifest, Path,
    PathBuf, WorkspaceState, current_lockfile::assert_loaded_current_lockfile_records,
    current_pnpmfiles, filesystem_now_ms, manifest_agreement::check_projects_content,
    project_structure_matches, settle_repeat_install,
};

/// `state` re-keyed onto `workspace_root` when it records these same
/// projects under one other root, or `None` when it records them in place or
/// no single rebase matches them. A pnpmfile outside that other root keeps
/// its path.
pub(super) fn relocated_state(
    state: &WorkspaceState,
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Option<WorkspaceState> {
    if !recorded_elsewhere(state, project_manifests) {
        return None;
    }
    let recorded_root = recorded_root(state)?;
    let rebase_onto_root = |path: &str| {
        rebase(Path::new(path), recorded_root, workspace_root)
            .map(|rebased| rebased.to_string_lossy().into_owned())
    };
    let projects = state.projects
        .iter()
        .map(|(dir, entry)| Some((rebase_onto_root(dir)?, entry.clone())))
        .collect::<Option<_>>()?;
    let pnpmfiles = state.pnpmfiles
        .iter()
        .map(|path| rebase_onto_root(path).unwrap_or_else(|| path.clone()))
        .collect();
    let relocated = WorkspaceState { projects, pnpmfiles, ..state.clone() };
    project_structure_matches(&relocated, project_manifests).then_some(relocated)
}

/// Whether no current project dir is a key of `state`.
pub(crate) fn recorded_elsewhere(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    !project_manifests
        .iter()
        .any(|(root_dir, _)| state.projects.contains_key(root_dir.to_string_lossy().as_ref()))
}

/// The shallowest recorded project dir, when it holds every other one.
fn recorded_root(state: &WorkspaceState) -> Option<&Path> {
    let root = state.projects
        .keys()
        .map(Path::new)
        .min_by_key(|dir| dir.components().count())?;
    state.projects
        .keys()
        .all(|dir| Path::new(dir).starts_with(root))
        .then_some(root)
}

/// `path` moved from under `from` to under `to`, or `None` outside `from`.
fn rebase(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    // `Path::join("")` would append a trailing separator to `to`.
    Some(if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) })
}

/// The fast path's verdict on a tree whose `state` was re-keyed by
/// [`relocated_state`]: up to date once [`prove_move`] holds and the state is
/// recorded where the tree is now.
pub(super) fn moved_tree_decision(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &ManifestDrift<'_>,
) -> Decision {
    let filesystem_now = rekeyed_validation_now(check, state, drift);
    match prove_move(check, drift)
        .and_then(|()| settle_repeat_install(check, state, None, filesystem_now, true))
    {
        Ok(()) => Decision::UpToDate,
        Err(reason) => Decision::Skipped { reason },
    }
}

/// The filesystem time a re-keyed state is validated at. A tree with nothing
/// newer than its last validation keeps that timestamp, so nothing is probed.
pub(super) fn rekeyed_validation_now(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &ManifestDrift<'_>,
) -> Option<i64> {
    if drift.modified().is_empty() && !drift.lockfile_modified {
        Some(state.last_validated_timestamp)
    } else {
        filesystem_now_ms(check.workspace_root)
    }
}

/// Read-only proof that a tree which moved with its project is the one this
/// checkout wants, and resolves everything from where it is now. Its
/// pnpmfiles require a full install to validate their content. `Err`
/// carries the reason it cannot be reused.
pub(super) fn prove_move(
    check: &OptimisticRepeatInstallCheck<'_>,
    drift: &ManifestDrift<'_>,
) -> Result<(), &'static str> {
    let &OptimisticRepeatInstallCheck {
        config,
        project_manifests,
        layout: crate::RepeatInstallLayout { node_linker, .. },
        ..
    } = check;
    if !current_pnpmfiles(check.workspace_root, config).is_empty() {
        return Err("a moved tree requires pnpmfile content validation");
    }
    // Both lockfiles are read here, and a refusal below falls through to the
    // full install, which reads the wanted one too, so the prefetch is never
    // spent for nothing: it parses while this thread does the rest.
    check.lockfile.prefetch();
    let wanted = validated_moved_lockfile(check)?;
    check_projects_content(
        check,
        wanted,
        &drift.stats.iter().collect::<Vec<_>>(),
        config.dedupe_peers,
    )?;
    if !crate::install::moved_tree_is_reusable(config, node_linker, project_manifests, wanted) {
        return Err("a bin in the moved tree names a path outside it");
    }
    Ok(())
}

fn validated_moved_lockfile<'a>(
    check: &'a OptimisticRepeatInstallCheck<'_>,
) -> Result<&'a Lockfile, &'static str> {
    let config = check.config;
    let node_linker = check.layout.node_linker;
    let modules = pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
        .ok()
        .flatten()
        .filter(|modules| {
            crate::install::modules_layout_consistent_with(modules, config, node_linker)
        })
        .ok_or("the moved tree's store or virtual store does not resolve from where it is")?;
    let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
        .map_err(|_| "the current lockfile cannot be loaded")?;
    let wanted = check.lockfile
        .get()
        .map_err(|_| "the wanted lockfile cannot be read or parsed")?
        .ok_or("a moved tree has no wanted lockfile to compare against")?;
    assert_loaded_current_lockfile_records(wanted, current.as_ref(), |current| {
        super::materialized_shape_matches(wanted, current, modules.included)
    })?;
    if let Some(current) = current.as_ref()
        && !crate::install::frozen_tree_intact(
            current,
            &modules,
            config,
            check.workspace_root,
            node_linker,
        )
    {
        return Err("the moved tree is missing an installed dependency");
    }
    Ok(wanted)
}

#[cfg(test)]
mod tests;
