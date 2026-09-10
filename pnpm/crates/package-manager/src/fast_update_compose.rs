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

    let FastUpdateDrift { importers, ignored, patched, settings: settings_drift } =
        detect_fast_update_drift(&DriftInputs {
            lockfile,
            manifests,
            project_manifests,
            prune_stale_importers,
            resolution_picks_lowest: config.resolution_mode.picks_lowest_direct(),
            ignored_optional_dependencies,
            patch_hashes,
            settings: &settings,
        })?;

    let mut candidate = lockfile.clone();
    apply_graph_drift(&mut candidate, &importers, &ignored, ignored_optional_dependencies)?;
    apply_patch_drift(&mut candidate, &patched, config.allow_unused_patches)?;
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

fn apply_patch_drift(
    candidate: &mut Lockfile,
    patched: &Drift<crate::fast_update_patched_dependencies::PatchedPlan>,
    allow_unused_patches: bool,
) -> Option<()> {
    if let Drift::Absorb(plan) = patched
        && !crate::fast_update_patched_dependencies::apply_patched_update(
            candidate,
            plan,
            allow_unused_patches,
        )
    {
        return None;
    }
    Some(())
}

fn apply_graph_drift(
    candidate: &mut Lockfile,
    importers: &Drift<crate::fast_update_importers::ImportersPlan<'_, '_>>,
    ignored: &Drift<()>,
    ignored_optional_dependencies: &[String],
) -> Option<()> {
    let mut edits = GraphEdits::default();
    if let Drift::Absorb(plan) = importers
        && !crate::fast_update_importers::apply_importers_update(candidate, plan, &mut edits)
    {
        return None;
    }
    if matches!(ignored, Drift::Absorb(())) {
        crate::fast_update_ignored_optional_dependencies::apply_ignored_optional_update(
            candidate,
            ignored_optional_dependencies,
            &mut edits,
        );
    }
    if !crate::fast_update_lockfile::finish_graph_edits(candidate, &edits) {
        return None;
    }
    Some(())
}

/// The inputs every drift detector reads.
struct DriftInputs<'a, 'manifest> {
    lockfile: &'a Lockfile,
    manifests: &'a [(String, &'manifest PackageManifest)],
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    prune_stale_importers: bool,
    resolution_picks_lowest: bool,
    ignored_optional_dependencies: &'a [String],
    patch_hashes: Option<&'a BTreeMap<String, String>>,
    settings: &'a pnpm_lockfile::LockfileSettings,
}

/// What each half of the lockfile drifted by, once every detector agrees the
/// drift can be absorbed without a resolution.
struct FastUpdateDrift<'a, 'manifest> {
    importers: Drift<crate::fast_update_importers::ImportersPlan<'a, 'manifest>>,
    ignored: Drift<()>,
    patched: Drift<crate::fast_update_patched_dependencies::PatchedPlan>,
    settings: Drift<()>,
}

/// `None` when any half needs a resolution, or when nothing drifted at all.
fn detect_fast_update_drift<'a, 'manifest>(
    inputs: &DriftInputs<'a, 'manifest>,
) -> Option<FastUpdateDrift<'a, 'manifest>> {
    let importers = match crate::fast_update_importers::detect_importers_drift(
        inputs.lockfile,
        inputs.manifests,
        inputs.project_manifests,
        inputs.prune_stale_importers,
        inputs.resolution_picks_lowest,
    ) {
        Drift::Resolve => return None,
        drift => drift,
    };
    let ignored =
        match crate::fast_update_ignored_optional_dependencies::detect_ignored_optional_drift(
            inputs.lockfile,
            inputs.ignored_optional_dependencies,
        ) {
            Drift::Resolve => return None,
            drift => drift,
        };
    let patched = match crate::fast_update_patched_dependencies::detect_patched_drift(
        inputs.lockfile,
        inputs.patch_hashes,
    ) {
        Drift::Resolve => return None,
        drift => drift,
    };
    let settings = match crate::fast_update_settings::detect_settings_drift(
        inputs.lockfile,
        inputs.settings,
    ) {
        Drift::Resolve => return None,
        drift => drift,
    };
    if matches!(
        (&importers, &ignored, &patched, &settings),
        (Drift::Clean, Drift::Clean, Drift::Clean, Drift::Clean),
    ) {
        return None;
    }
    Some(FastUpdateDrift { importers, ignored, patched, settings })
}

#[cfg(test)]
mod tests;
