use super::{
    BuildOneSnapshot, PackageKey, RebuildOptions, allow_build_key_from_ignored_build,
    get_pkg_id_with_patch_hash,
};

/// An explicit `pacquet rebuild` re-runs the build scripts of the selected
/// packages even when the side-effects cache reports them already built. The
/// selection holds allow-build keys (the package name for registry deps, the
/// full pkgId for git/tarball artifacts), so either form matches — a selected
/// non-registry artifact is forced past the `is_built` gate too.
pub(super) fn rebuild_forces_build(
    rebuild: Option<&RebuildOptions>,
    name: &str,
    dep_path: &str,
) -> bool {
    rebuild.is_some_and(|rebuild| {
        rebuild.is_selected(name)
            || rebuild.is_selected(&allow_build_key_from_ignored_build(dep_path))
    })
}

/// Whether this snapshot's lifecycle scripts run at all.
///
/// The allow-policy gate still applies — a rebuild never builds a disallowed
/// package. And a `pacquet rebuild <pkg>` runs scripts only for the selected
/// packages: non-selected ones are still evaluated by the policy gate (so
/// their `.modules.yaml` ignored-builds record stays intact), but their
/// scripts are suppressed here. The side-effects `is_built` gate is only an
/// optimization and is disabled by default, so this gate — not that
/// short-circuit — is what bounds script execution to the selection.
pub(super) fn snapshot_runs_scripts(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    dep_path: &str,
    build: (bool, bool),
) -> bool {
    let (requires_build, force_rebuild) = build;
    if !requires_build || context.scripts.ignore {
        return false;
    }
    if context.rebuild.is_some() && !force_rebuild {
        return false;
    }
    scripts_are_allowed(context, snapshot_key, dep_path)
}

/// The `allowBuilds` gate, which only applies when the node has scripts to
/// run. A patched-only package skips this check entirely and proceeds to patch
/// application; a refusal returns `false` rather than failing, so the patch
/// still gets applied even when scripts are disallowed.
fn scripts_are_allowed(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    dep_path: &str,
) -> bool {
    if let Some(allowed) = context.allow_build_policy.check(dep_path) {
        allowed
    } else {
        {
            // Poison-recover: see the equivalent call site at the end of
            // `BuildModules::run` for the safety argument (BTreeSet insertion
            // is atomic from the data-structure's POV).
            //
            // The patch hash is kept: two copies of a package that differ only
            // by an applied patch are different builds to approve, and pnpm's
            // `dedupePackageNamesFromIgnoredBuilds` reports them apart for the
            // same reason. `dep_path` has already lost it, so it is re-derived
            // from the full key.
            let ignored_key = get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string();
            context.progress.ignored_builds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(ignored_key);
            false
        }
    }
}
