use crate::fast_update_lockfile::GraphEdits;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use std::{collections::BTreeMap, path::PathBuf};

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

/// Rewrite the loaded lockfile in place of a full resolution, composing
/// every handler with absorbable drift onto one shared candidate — no
/// handler requires being the only change. Removals apply first, then
/// the shared graph epilogue
/// ([`crate::fast_update_lockfile::finish_graph_edits`]), then patch
/// rekeying — after the prune, so its guards see the packages a full
/// resolution would see — and the settings block last. What survives is
/// then held to
/// [`crate::fast_update_patched_dependencies::every_configured_patch_is_applied`].
///
/// `None` — no drift, or drift some handler cannot express — leaves the
/// caller on the full-resolution path; the caller still validates the
/// candidate with the freshness gates before committing.
pub(crate) fn try_compose_fast_updates(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
    project_manifests: &[(PathBuf, &PackageManifest)],
    config: &Config,
    patch_hashes: Option<&BTreeMap<String, String>>,
    prune_stale_importers: bool,
) -> Option<Lockfile> {
    let ignored_optional_dependencies =
        config.ignored_optional_dependencies.as_deref().unwrap_or_default();
    let settings = crate::fast_update_settings::lockfile_settings_from_config(config);

    let importers = match crate::fast_update_importers::detect_importers_drift(
        lockfile,
        manifests,
        project_manifests,
        prune_stale_importers,
        config.resolution_mode.picks_lowest_direct(),
    ) {
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
        match crate::fast_update_patched_dependencies::detect_patched_drift(lockfile, patch_hashes)
        {
            Drift::Resolve => return None,
            drift => drift,
        };
    let settings_drift =
        match crate::fast_update_settings::detect_settings_drift(lockfile, &settings) {
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
    if matches!(settings_drift, Drift::Absorb(()))
        && !crate::fast_update_settings::apply_settings_update(
            &mut candidate,
            &settings,
            project_manifests,
        )
    {
        return None;
    }
    if !crate::fast_update_patched_dependencies::every_configured_patch_is_applied(
        &candidate,
        patch_hashes,
        config.allow_unused_patches,
    ) {
        return None;
    }
    Some(candidate)
}

#[cfg(test)]
mod tests;
