use super::super::{
    Arc, BTreeMap, Config, HashSet, InstallError, Lockfile, LogEvent, LogLevel, NodeLinker,
    PackageManifest, Path, PathBuf, PnpmLog, RebuildOptions, Reporter, ResolutionVerifier,
    map_frozen_lockfile_error, record_lockfile_verified, verify_lockfile_eagerly,
};

/// Whether `allowBuilds` moved since the previous install in a way the
/// hoisted linker must act on: a build it ignored is now allowed, or one
/// it ran is no longer allowed. Read from the previous `.modules.yaml`;
/// `false` on a first install.
pub(super) fn allow_builds_changed_since(
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
    config: &pnpm_config::Config,
) -> bool {
    modules_manifest.is_some_and(|modules| {
        super::super::has_newly_allowed_ignored_builds(modules, config)
            || super::super::has_revoked_allowed_builds(modules, config)
            || recorded_allow_builds_differ(modules, config)
    })
}
/// Whether the `allowBuilds` entries the previous install recorded differ
/// from the current setting: an entry flipped between `true` and `false`,
/// or one added or removed. The two predicates above see an ignored build
/// becoming allowed and an approval being withdrawn; this sees the
/// remaining transitions, such as an explicit `false` becoming `true`,
/// which leaves no ignored entry behind to notice. Placeholder entries the
/// approval scaffold writes carry no decision and are ignored.
pub(super) fn recorded_allow_builds_differ(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &pnpm_config::Config,
) -> bool {
    let recorded: std::collections::HashMap<&str, bool> = modules
        .allow_builds
        .iter()
        .flatten()
        .filter_map(|(spec, value)| match value {
            pnpm_modules_yaml::AllowBuildValue::Bool(decision) => Some((spec.as_str(), *decision)),
            pnpm_modules_yaml::AllowBuildValue::String(_) => None,
        })
        .collect();
    recorded.len() != config.allow_builds.len()
        || recorded.iter().any(|(spec, decision)| config.allow_builds.get(*spec) != Some(decision))
}
/// The `name@version` keys the previous install's `.modules.yaml` recorded
/// as not built, its `ignoredBuilds` and `pendingBuilds`. Empty on a first
/// install.
pub(super) fn prior_unbuilt_builds(
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> pnpm_deps_restorer::UnbuiltBuilds {
    let mut unbuilt = pnpm_deps_restorer::UnbuiltBuilds::new();
    if let Some(modules) = modules_manifest {
        unbuilt.extend(modules.pending_builds.iter().cloned());
        unbuilt.extend(
            modules.ignored_builds.iter().flatten().map(|dep_path| dep_path.as_str().to_string()),
        );
    }
    unbuilt
}
/// The project manifests whose importer the frozen install anchors on.
pub(super) fn anchored_project_manifests<'a>(
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    workspace_root: &Path,
    project_anchor_ids: &HashSet<String>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            project_anchor_ids
                .contains(&pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        })
        .cloned()
        .collect()
}
pub(super) fn importer_manifests_by_id<'a>(
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    workspace_root: &Path,
) -> BTreeMap<String, &'a PackageManifest> {
    project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
        })
        .collect()
}
pub(super) fn lockfile_specifier_manifests_by_id(
    project_manifests: Vec<(PathBuf, PackageManifest)>,
    workspace_root: &Path,
) -> BTreeMap<String, PackageManifest> {
    project_manifests
        .into_iter()
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(workspace_root, &project_dir), manifest)
        })
        .collect()
}
// Record under the same path the verification gates key
// their cache on, so the next install's stat shortcut hits.
pub(super) fn record_fresh_lockfile_verified(
    result: &crate::InstallWithFreshLockfileResult,
    derived_lockfile_path: Option<PathBuf>,
    site: (&Path, &Config),
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) {
    let (workspace_root, config) = site;
    if !result.can_record_lockfile_verification {
        return;
    }
    let Some(lockfile) = result.wanted_lockfile.as_ref() else { return };
    let lockfile_path =
        derived_lockfile_path.unwrap_or_else(|| workspace_root.join(config.wanted_lockfile_name()));
    record_lockfile_verified(
        Some(&config.cache_dir),
        &lockfile_path,
        lockfile,
        resolution_verifiers,
    );
}
/// A selected (`--filter`) frozen install verifies the whole lockfile up
/// front, so nothing is left for the concurrent gate to carry; an unselected
/// one hands its override straight through.
pub(super) async fn settle_frozen_verification<'install, Reporter: self::Reporter>(
    requested_importer_ids: Option<&HashSet<String>>,
    verification_override: Option<super::super::LockfileVerificationOverride<'install>>,
    lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
    derived_lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<Option<super::super::LockfileVerificationOverride<'install>>, InstallError> {
    if requested_importer_ids.is_none() {
        return Ok(verification_override);
    }
    match verification_override {
        Some(verification_override) => {
            verification_override.await.map_err(map_frozen_lockfile_error)?;
        }
        None => {
            verify_lockfile_eagerly::<Reporter>(
                lockfile,
                resolution_verifiers,
                derived_lockfile_path,
                cache_dir,
            )
            .await?;
        }
    }
    Ok(None)
}
/// The importers whose own project manifests the frozen install anchors on.
pub(super) fn frozen_project_anchor_ids(
    requested_importer_ids: Option<&HashSet<String>>,
    real_importer_ids: &HashSet<String>,
    node_linker: NodeLinker,
    materialization: Option<&crate::MaterializationClosure>,
) -> HashSet<String> {
    match requested_importer_ids {
        Some(selected) if matches!(node_linker, NodeLinker::Hoisted) => selected.clone(),
        Some(_) => materialization
            .expect("selected install has a materialization closure")
            .importer_ids
            .clone(),
        None => real_importer_ids.clone(),
    }
}
/// The importers a frozen install materializes first. A hoisted linker shares
/// one tree, so a selected install still has to materialize every importer.
pub(super) fn initial_materialization_ids(
    lockfile: &Lockfile,
    requested_importer_ids: Option<&HashSet<String>>,
    node_linker: NodeLinker,
) -> Option<HashSet<String>> {
    let selected = requested_importer_ids?;
    if matches!(node_linker, NodeLinker::Hoisted) {
        return Some(lockfile.importers.keys().cloned().collect());
    }
    Some(selected.clone())
}
/// pnpm's headless installer announces itself whenever it is entered — also
/// on a cold `node_modules` and on subset (`--filter`) installs — not only
/// when nothing needs to be materialized.
///
/// `importing_only` gets upstream's `ignorePackageManifest` wording instead;
/// `pnpm fetch` is the one caller combining `ignore_manifest_check` with a
/// non-full install, and the flag alone can't identify it because `install
/// --ignore-manifest-check` is a user-facing way to skip the frozen freshness
/// gate on a full install. Upstream's headless entry returns before the
/// announcement for an empty lockfile (`isEmptyLockfile`), and an explicit
/// `pnpm rebuild` is not an install, so both stay silent.
pub(super) fn announce_headless_install<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    rebuild: Option<&RebuildOptions>,
    importing_only: bool,
    prefix: &str,
) {
    if rebuild.is_some() || lockfile.is_empty() {
        return;
    }
    let message = if importing_only {
        "Importing packages to virtual store"
    } else {
        "Lockfile is up to date, resolution step is skipped"
    };
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
}
