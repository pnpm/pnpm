use crate::fast_update_lockfile::GraphEdits;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::PackageManifest;
use std::path::PathBuf;

/// What a fast-update handler's detector concluded about its slice of
/// the configuration and manifests.
pub(crate) enum Drift<Plan> {
    /// Nothing changed; the handler has nothing to do.
    Clean,
    /// Something changed and the handler can rewrite the lockfile to
    /// match, replaying `Plan` onto the shared candidate.
    Absorb(Plan),
    /// Something changed that only a resolution can express.
    Resolve,
}

/// Rewrite the loaded lockfile in place of a full resolution for the
/// drift the lockfile itself proves is safe to absorb, composing every
/// applicable handler onto one candidate: manifest drift (compatible
/// range changes, dependency group moves, removals), a widened
/// `ignoredOptionalDependencies`, a changed `patchedDependencies`, and
/// a setting change that cannot affect the recorded graph. Drift that
/// spans several of those dimensions at once is absorbed in one pass —
/// no handler requires being the only change.
///
/// The handlers run in a fixed order. Removals land first (manifest
/// drift, then newly ignored optional dependencies), then the shared
/// epilogue prunes what nothing reaches and refuses candidates whose
/// surviving peer suffixes embed a dropped package. Patches rekey after
/// that, so the unused-patch guard and the rekey plan see the packages
/// a full resolution would see. The settings block, which touches no
/// graph, is recorded last.
///
/// `None` — no drift, or drift some handler cannot express — leaves the
/// caller on the full-resolution path. The caller still validates the
/// candidate with the lockfile freshness gates before committing, so a
/// handler that rewrites too much falls back rather than corrupting.
pub(crate) fn try_compose_fast_updates(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
    project_manifests: &[(PathBuf, &PackageManifest)],
    config: &Config,
) -> Option<Lockfile> {
    let ignored_optional_dependencies =
        config.ignored_optional_dependencies.as_deref().unwrap_or_default();
    let settings = crate::fast_update_settings::lockfile_settings_from_config(config);

    let importers = match crate::fast_update_importers::detect_importers_drift(lockfile, manifests)
    {
        Drift::Resolve => return None,
        drift => drift,
    };
    let ignored =
        match crate::fast_update_ignored_optional_dependencies::detect_ignored_optional_drift(
            lockfile,
            ignored_optional_dependencies,
        ) {
            Drift::Resolve => return None,
            drift => drift,
        };
    let patched =
        match crate::fast_update_patched_dependencies::detect_patched_drift(lockfile, config) {
            Drift::Resolve => return None,
            drift => drift,
        };
    let settings_drift = match crate::fast_update_settings::detect_settings_drift(
        lockfile,
        &settings,
        project_manifests,
    ) {
        Drift::Resolve => return None,
        drift => drift,
    };
    if matches!(
        (&importers, &ignored, &patched, &settings_drift),
        (Drift::Clean, Drift::Clean, Drift::Clean, Drift::Clean),
    ) {
        return None;
    }

    let mut candidate = lockfile.clone();
    let mut edits = GraphEdits::default();
    if let Drift::Absorb(plan) = &importers
        && !crate::fast_update_importers::apply_importers_update(&mut candidate, plan, &mut edits)
    {
        return None;
    }
    if matches!(ignored, Drift::Absorb(())) {
        crate::fast_update_ignored_optional_dependencies::apply_ignored_optional_update(
            &mut candidate,
            ignored_optional_dependencies,
            &mut edits,
        );
    }
    if !crate::fast_update_lockfile::finish_graph_edits(&mut candidate, &edits) {
        return None;
    }
    if let Drift::Absorb(plan) = &patched
        && !crate::fast_update_patched_dependencies::apply_patched_update(
            &mut candidate,
            plan,
            config.allow_unused_patches,
        )
    {
        return None;
    }
    if matches!(settings_drift, Drift::Absorb(())) {
        crate::fast_update_settings::apply_settings_update(&mut candidate, &settings);
    }
    Some(candidate)
}

#[cfg(test)]
mod tests;
