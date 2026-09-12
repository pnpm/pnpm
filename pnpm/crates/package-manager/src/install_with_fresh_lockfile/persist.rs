use super::{
    InstallWithFreshLockfileResult, errors::InstallWithFreshLockfileError,
    seed_policy::UpdateSeedPolicy,
};
use crate::{HoistedDependencies, SkippedSnapshots};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_resolving_resolver_base::ResolutionVerifier;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::Arc,
};

pub(super) async fn verify_repair_if_filtered<Reporter: pnpm_reporter::Reporter>(
    verify_filtered_repair: bool,
    built_lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<(), InstallWithFreshLockfileError> {
    if !verify_filtered_repair {
        return Ok(());
    }
    verify_merged_repair::<Reporter>(built_lockfile, resolution_verifiers).await
}
/// The repair copy `pacquet install --fix-lockfile` resolves against, which
/// drops what the repair is meant to rebuild.
pub(super) fn fix_lockfile_copy(
    update_seed_policy: &UpdateSeedPolicy,
    wanted_lockfile: Option<&Lockfile>,
) -> Option<Lockfile> {
    if !matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile) {
        return None;
    }
    wanted_lockfile.cloned().map(|mut lockfile| {
        lockfile.prepare_for_fix();
        lockfile
    })
}
/// The built wanted lockfile whenever lockfiles are enabled, whether or
/// not this run wrote it, and whether a verification may be recorded
/// against the file on disk, which only a written one allows.
pub(super) struct PersistedLockfile {
    pub(super) lockfile: Option<Lockfile>,
    pub(super) can_record_lockfile_verification: bool,
}
/// Save `pnpm-lock.yaml` after the build phase succeeds, so a partial install
/// can't leave a lockfile pointing at slots that never landed on disk.
pub(super) async fn persist_fresh_lockfile(
    built_lockfile: Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    save_lockfile: bool,
    after_all_resolved: (Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>, Option<pnpm_hooks::LogFn>),
) -> Result<PersistedLockfile, InstallWithFreshLockfileError> {
    if !config.lockfile {
        return Ok(PersistedLockfile { lockfile: None, can_record_lockfile_verification: false });
    }
    if !save_lockfile {
        // Nothing was persisted, so there is no `pnpm-lock.yaml` whose
        // verification a later install could key off.
        return Ok(PersistedLockfile {
            lockfile: Some(built_lockfile),
            can_record_lockfile_verification: false,
        });
    }
    let (hook, log) = after_all_resolved;
    let target = lockfile_dir.join(config.wanted_lockfile_name());
    let can_record_lockfile_verification =
        save_wanted_lockfile(&built_lockfile, &target, hook, log).await?;
    Ok(PersistedLockfile { lockfile: Some(built_lockfile), can_record_lockfile_verification })
}
/// Importers whose linked workspace dependency declares
/// `peerDependencies`.
///
/// The lockfile walk that renders the report reads a linked project's
/// manifest and checks its peers against the *consuming* importer's
/// dependencies. Peer resolution has no counterpart for that check — a
/// `link:` node's own peers are the linked importer's business — so
/// these importers never reach
/// `peer_dependency_issues_by_importer` and have to join the report's
/// candidate set on their own. Only the direct consumer is needed: it
/// is the importer whose dependencies the check compares against.
///
/// Answered from the manifests the install already parsed, so a
/// workspace whose projects declare no peers costs one pass over the
/// declared dependencies and no I/O. Over-approximates — a
/// `workspace:` dependency the lockfile records as an injected
/// directory rather than a link still counts, as does a target this
/// cannot name — which only widens the walk.
pub(super) fn importers_consuming_linked_peers(
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    lockfile_dir: &Path,
) -> HashSet<String> {
    let declares_peers = |manifest: &PackageManifest| {
        manifest
            .value()
            .get("peerDependencies")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|peers| !peers.is_empty())
    };
    fn project_name(manifest: &PackageManifest) -> Option<&str> {
        manifest.value().get("name")?.as_str()
    }
    let scan = LinkedPeerScan {
        importer_manifests,
        lockfile_dir,
        peer_declaring_ids: importer_manifests
            .iter()
            .filter(|(_, manifest)| declares_peers(manifest))
            .map(|(importer_id, _)| importer_id.as_str())
            .collect(),
        peer_declaring_names: importer_manifests
            .values()
            .filter(|manifest| declares_peers(manifest))
            .filter_map(|manifest| project_name(manifest))
            .collect(),
        project_names: importer_manifests
            .values()
            .filter_map(|manifest| project_name(manifest))
            .collect(),
    };

    let mut consumers = HashSet::new();
    for (importer_id, manifest) in importer_manifests {
        let importer_dir = lockfile_dir.join(importer_id);
        let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        let consumes = manifest.dependencies(groups).any(|(entry_key, bare_specifier)| {
            linked_target_may_declare_peers(&scan, &importer_dir, entry_key, bare_specifier)
        });
        if consumes {
            consumers.insert(importer_id.clone());
        }
    }
    consumers
}
/// The workspace projects a link target is matched against.
pub(super) struct LinkedPeerScan<'a> {
    importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    lockfile_dir: &'a Path,
    peer_declaring_ids: HashSet<&'a str>,
    peer_declaring_names: HashSet<&'a str>,
    project_names: HashSet<&'a str>,
}
/// Whether one declared dependency links to a project that may declare peers.
///
/// A target whose peers are unknown here counts. The walk reads such a
/// manifest when it resolves inside the lockfile directory and skips one that
/// escapes; symlinks decide which, so both count.
pub(super) fn linked_target_may_declare_peers(
    scan: &LinkedPeerScan<'_>,
    importer_dir: &Path,
    entry_key: &str,
    bare_specifier: &str,
) -> bool {
    if let Some(spec) = pnpm_workspace_spec::WorkspaceSpec::parse(bare_specifier) {
        // `workspace:<name>@<range>` names the project it links
        // to; the bare form takes that name from the entry key.
        // The range picks among the projects sharing that name,
        // so any one of them declaring a peer counts — reaching
        // for the picked version would duplicate
        // `resolve_workspace_range` to narrow an answer that is
        // only ever "walk this importer too".
        let linked_name = spec.alias.as_deref().unwrap_or(entry_key);
        return scan.peer_declaring_names.contains(linked_name)
            || !scan.project_names.contains(linked_name);
    }
    // Only `file:` reads the name: it resolves to a package
    // when that names a tarball, and only its directory form
    // becomes the `link:` entry the walk inspects. A `link:`
    // is a directory whatever it is called.
    let Some(relative) = bare_specifier.strip_prefix("link:").or_else(|| {
        bare_specifier
            .strip_prefix("file:")
            .filter(|_| !pnpm_resolving_local_resolver::is_tarball_filename(bare_specifier))
    }) else {
        return false;
    };
    let linked_id =
        pnpm_workspace::importer_id_from_root_dir(scan.lockfile_dir, &importer_dir.join(relative));
    scan.peer_declaring_ids.contains(linked_id.as_str())
        || !scan.importer_manifests.contains_key(&linked_id)
}
pub(super) struct LockfileOnlyOptions<'a> {
    pub(super) built_lockfile: Lockfile,
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) config: &'a Config,
    pub(super) lockfile_dir: &'a Path,
    pub(super) requester: &'a str,
    pub(super) dry_run: bool,
    pub(super) save_lockfile: bool,
    pub(super) after_all_resolved_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) after_all_resolved_log: Option<pnpm_hooks::LogFn>,
    pub(super) store_index_writer: Arc<pnpm_store_dir::StoreIndexWriter>,
    pub(super) writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}
pub(super) async fn verify_merged_repair<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<(), InstallWithFreshLockfileError> {
    pnpm_lockfile_verification::verify_lockfile_resolutions::<Reporter>(
        lockfile,
        resolution_verifiers,
        &pnpm_lockfile_verification::VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .map_err(InstallWithFreshLockfileError::LockfileVerification)
}
/// Tail of the `--lockfile-only` path: persist the freshly-built
/// lockfile, close the store-index writer, and report the install done.
///
/// `--dry-run` builds the would-be lockfile so the caller can diff it,
/// but never persists it. A plain `--lockfile-only` writes it (unless
/// `lockfile: false`). Nothing was materialized, so no build phase ran
/// and nothing was ignored, deferred, or skipped.
pub(super) async fn finish_lockfile_only<Reporter: self::Reporter>(
    opts: LockfileOnlyOptions<'_>,
) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
    let (wanted_lockfile, can_record_lockfile_verification) = if opts.dry_run || !opts.save_lockfile
    {
        (Some(opts.built_lockfile), false)
    } else if opts.config.lockfile {
        let can_record_lockfile_verification = save_wanted_lockfile(
            &opts.built_lockfile,
            &opts.lockfile_dir.join(opts.config.wanted_lockfile_name()),
            opts.after_all_resolved_hook,
            opts.after_all_resolved_log,
        )
        .await?;
        (Some(opts.built_lockfile), can_record_lockfile_verification)
    } else {
        (None, false)
    };

    // Close the writer cleanly even though no rows were written,
    // mirroring the materializing path: drop closes the channel, the
    // caller awaits the returned task after its own tail writes.
    drop(opts.store_index_writer);

    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: opts.requester.to_string(),
        stage: Stage::ImportingDone,
    }));
    Ok(InstallWithFreshLockfileResult {
        hoisted_dependencies: HoistedDependencies::new(),
        hoisted_locations: BTreeMap::new(),
        injected_deps: BTreeMap::new(),
        peer_issue_importer_ids: opts.peer_issue_importer_ids,
        wanted_lockfile,
        can_record_lockfile_verification,
        ignored_builds: Vec::new(),
        deferred_builds: Vec::new(),
        skipped: SkippedSnapshots::new(),
        store_index_teardown: opts.writer_task,
    })
}
pub(super) async fn save_wanted_lockfile(
    built_lockfile: &Lockfile,
    target: &Path,
    hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    log: Option<pnpm_hooks::LogFn>,
) -> Result<bool, InstallWithFreshLockfileError> {
    let Some(hook) = hook else {
        built_lockfile
            .save_to_path(target)
            .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
        return Ok(true);
    };

    let value = serde_json::to_value(built_lockfile)
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedSerialize)?;
    let ctx = pnpm_hooks::HookContext { log: log.unwrap_or_else(|| Arc::new(|_| {})), dir: None };
    let result = hook
        .after_all_resolved(value, ctx)
        .await
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedHook)?;

    // `Null` means the pnpmfile has no `afterAllResolved` hook, so write the
    // typed lockfile unchanged.
    if result.is_null() {
        built_lockfile.save_to_path(target)
    } else {
        pnpm_lockfile::save_value_to_path(&result, target)
    }
    .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
    Ok(result.is_null())
}
