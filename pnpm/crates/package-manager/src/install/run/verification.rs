use std::{
    path::PathBuf,
    sync::Arc,
};

use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_npm_resolver::InMemoryPackageMetaCache;

use super::{
    InstallError,
    InstallScope,
    Lockfiles,
    RunExecution,
    lockfile_load::Loaded,
    workspace::workspace_projects,
};
use crate::{
    build_resolution_verifiers,
    verified_project_workspace_dirs,
};

// One per-install packument cache shared with both the
// lockfile-verifier (below) and the resolver in
// `install_with_fresh_lockfile` (further down). The
// single instance lets a name the resolver fetched during this
// install short-circuit the verifier's own fetch chain, and
// vice versa.
// `trust_lockfile` leaves the verifier list empty and sets `exotic_subdeps`
// to `Allow`, making every policy gate a no-op, fresh local resolution is
// already filtered by the resolver's own per-version gate
// (`minimumReleaseAge` via `ResolveResult::policy_violation`,
// `trustPolicy='no-downgrade'` via the npm resolver's
// `fail_if_trust_downgraded_for_pick`). Verifiers and `exotic_subdeps` are
// built whenever untrusted, independent of whether a lockfile is loaded, so
// the fresh-resolve path can record the freshly written lockfile as
// already-verified (see `record_lockfile_verified_by_content`).
// Shared with `CreateVirtualStore`, which fills it after its
// warm/cold partition so the verifier's age gate can lean on
// this install's own canonical tarball fetches instead of a
// metadata body per entry.
pub(in crate::install) struct DerivedLockfileSource {
    pub(in crate::install) path: PathBuf,
    pub(in crate::install) stat: Option<pnpm_lockfile_verification::LockfileStat>,
}

impl DerivedLockfileSource {
    pub(in crate::install) fn as_cache_source(
        &self,
    ) -> pnpm_lockfile_verification::LockfileCacheSource<'_> {
        pnpm_lockfile_verification::LockfileCacheSource {
            path: self.path.as_path(),
            stat: self.stat.as_ref(),
        }
    }
}

pub(in crate::install) struct Verification {
    pub(in crate::install) meta_cache: Arc<InMemoryPackageMetaCache>,
    pub(in crate::install) planned_canonical_fetches:
        pnpm_resolving_resolver_base::PlannedCanonicalFetches,
    pub(in crate::install) resolution_verifiers: Vec<Arc<dyn super::super::ResolutionVerifier>>,
    pub(in crate::install) exotic_subdeps: pnpm_lockfile_verification::ExoticSubdepsPolicy,
    pub(in crate::install) local_policy: pnpm_lockfile_verification::LocalResolutionPolicy,
    pub(in crate::install) derived_lockfile: Option<DerivedLockfileSource>,
    /// The record an `autoDedupe` `--lockfile-only` install that resolves the
    /// whole workspace writes once it saves the lockfile, and that a later one
    /// checks. `None` for every other install.
    pub(in crate::install) auto_dedupe_baseline:
        Option<super::super::auto_dedupe_baseline::AutoDedupeBaseline>,
}

impl Verification {
    pub(in crate::install) fn cache_source(
        &self,
    ) -> Option<pnpm_lockfile_verification::LockfileCacheSource<'_>> {
        self.derived_lockfile.as_ref().map(DerivedLockfileSource::as_cache_source)
    }

    pub(super) fn set_up(
        execution: &RunExecution<'_>,
        loaded: &Loaded<'_>,
        lockfiles: &Lockfiles<'_>,
        (scope, project_manifests): (&InstallScope<'_>, &[(PathBuf, &PackageManifest)]),
    ) -> Result<Self, InstallError> {
        let install = execution.install;
        let owned = &execution.owned;
        let shared_caches = owned.shared_caches();
        let meta_cache: Arc<InMemoryPackageMetaCache> =
            shared_caches.map_or_else(Default::default, |caches| Arc::clone(&caches.packuments));
        let planned_canonical_fetches =
            pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
        let resolution_verifiers = install_resolution_verifiers(
            install.context.config,
            install.lockfile_policy.trust,
            (&owned.http_client_arc, &meta_cache, owned.resolution.auth_override.as_ref()),
            (&planned_canonical_fetches, shared_caches),
        )?;
        let exotic_subdeps = build_run_exotic_subdeps_policy(execution, scope, project_manifests);
        let local_policy = build_local_policy(
            execution,
            loaded.pnpmfile_hook.is_some(),
            (scope, project_manifests),
        );
        let derived_lockfile = lockfiles.wanted
            .matches_single_on_disk_file(loaded)
            .then(|| {
                let path = loaded.wanted.loaded_path?.to_path_buf();
                let stat = loaded.wanted.loaded_metadata.map(
                    pnpm_lockfile_verification::stat_from_metadata,
                );
                Some(DerivedLockfileSource { path, stat })
            })
            .flatten();
        Ok(Self {
            meta_cache,
            planned_canonical_fetches,
            resolution_verifiers,
            exotic_subdeps,
            local_policy,
            derived_lockfile,
            auto_dedupe_baseline: execution.auto_dedupe_baseline(scope, project_manifests),
        })
    }

    pub(super) async fn enrich_with_pnpmfile_hook(
        &mut self,
        wanted: Option<&pnpm_lockfile::Lockfile>,
        config: &Config,
        http_client: &pnpm_network::ThrottledClient,
        hook: Option<&dyn pnpm_hooks::PnpmfileHooks>,
    ) -> Result<(), InstallError> {
        if let Some(wanted) = wanted {
            let current = std::mem::take(&mut self.local_policy);
            self.local_policy = pnpm_deps_restorer::enrich_local_policy_for_install(
                current,
                wanted,
                pnpm_deps_restorer::LocalPolicyEnrichDrivers {
                    config: Some(config),
                    http_client: Some(http_client),
                    hook,
                },
            )
            .await
            .map_err(InstallError::from)?;
        }
        Ok(())
    }

    pub(super) fn take_valid_override<'install, Reporter: super::super::Reporter + 'static>(
        &self,
        wanted: &super::wanted::WantedLockfile<'_>,
        verification: &mut crate::install::InstallVerificationOptions<'install>,
    ) -> Option<crate::LockfileVerificationOverride<'install>> {
        let override_check = verification.override_check.take()?;
        let Some(recorded) = verification.prepared_policy.take() else {
            return Some(self.chain_hooked_verification::<Reporter>(override_check, wanted));
        };
        pnpm_lockfile_verification::can_trust_prepared_policy(
            &recorded,
            &self.resolution_verifiers,
            &self.exotic_subdeps,
        )
        .then_some(override_check)
    }

    pub(super) fn sync_derived_lockfile_path(
        &mut self,
        wanted: &super::wanted::WantedLockfile<'_>,
        loaded: &Loaded<'_>,
    ) {
        if !wanted.matches_single_on_disk_file(loaded) {
            self.derived_lockfile = None;
        }
    }

    fn chain_hooked_verification<'install, Reporter: super::super::Reporter + 'static>(
        &self,
        override_check: crate::LockfileVerificationOverride<'install>,
        wanted: &super::wanted::WantedLockfile<'_>,
    ) -> crate::LockfileVerificationOverride<'install> {
        if !wanted.is_hooked()
            || (self.resolution_verifiers.is_empty() && !self.exotic_subdeps.is_enforced())
        {
            return override_check;
        }
        let Some(lockfile) = wanted.get().cloned() else {
            return override_check;
        };
        let verifiers = self.resolution_verifiers.clone();
        let exotic_subdeps = self.exotic_subdeps.clone();
        let local_policy = self.local_policy.clone();
        Box::pin(async move {
            override_check.await?;
            pnpm_lockfile_verification::verify_lockfile_resolutions::<Reporter>(
                &lockfile,
                &verifiers,
                &pnpm_lockfile_verification::VerifyLockfileResolutionsOptions {
                    exotic_subdeps,
                    local_policy: Some(local_policy),
                    ..Default::default()
                },
            )
            .await
            .map_err(pnpm_deps_restorer::InstallFrozenLockfileError::LockfileVerification)
        })
    }
}

fn build_local_policy(
    execution: &RunExecution<'_>,
    has_pnpmfile_hook: bool,
    (scope, project_manifests): (&InstallScope<'_>, &[(PathBuf, &PackageManifest)]),
) -> pnpm_lockfile_verification::LocalResolutionPolicy {
    let config = execution.install.context.config;
    let overrides_root = execution.options.manifests.overrides_root_dir;
    let containment_root =
        overrides_root.unwrap_or(&execution.workspace.dirs.workspace_manifest_dir);
    let base = pnpm_deps_restorer::local_resolution_policy_for_workspace_with_catalogs(
        config,
        Some(&execution.workspace.catalogs),
        (
            overrides_root,
            containment_root,
            config.lockfile_dir_for(&execution.workspace.dirs.workspace_root),
        ),
        has_pnpmfile_hook,
    )
    .with_deploy(matches!(execution.install.execution.mutation, crate::ProjectMutation::Deploy));
    if execution.options.manifests.deploy_hook
        || execution.install.lockfile_policy.ignore_manifest_check
    {
        return base;
    }
    let with_live = pnpm_deps_restorer::attach_live_project_manifests(
        base,
        project_manifests
            .iter()
            .chain(scope.workspace_projects.iter().flatten()),
    );
    pnpm_deps_restorer::attach_selected_project_manifests(with_live, project_manifests)
}

fn build_run_exotic_subdeps_policy(
    execution: &RunExecution<'_>,
    scope: &InstallScope<'_>,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> pnpm_lockfile_verification::ExoticSubdepsPolicy {
    let install = execution.install;
    let effective_trust =
        execution.options.save.recorded_trust_lockfile.unwrap_or(install.lockfile_policy.trust);
    if effective_trust || !install.context.config.block_exotic_subdeps {
        return pnpm_lockfile_verification::ExoticSubdepsPolicy::Allow;
    }
    let all_projects = workspace_projects(
        execution.loaded_workspace_projects,
        execution.options.selection.as_ref(),
    );
    let discovered_manifests = project_manifests
        .iter()
        .map(|(dir, manifest)| (dir.as_path(), manifest.value()))
        .chain(
            all_projects
                .into_iter()
                .flatten()
                .map(|project| (project.root_dir.as_path(), project.manifest.value())),
        );
    let verified_dirs = verified_project_workspace_dirs(
        &execution.workspace.dirs.workspace_root,
        discovered_manifests,
    );
    let extra_dirs = verified_dirs
        .iter()
        .map(String::as_str)
        .chain(execution.options.verification.extra_workspace_dirs.iter().map(String::as_str));
    super::super::config_exotic_subdeps_policy(
        install.context.config,
        Some(scope.importers.real_importer_ids.iter().map(String::as_str)),
        extra_dirs,
    )
}

/// Resolution verifiers re-apply `minimumReleaseAge` /
/// `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper check) to
/// every entry in the loaded `pnpm-lock.yaml`. `trust_lockfile` (the opt-out
/// for environments that treat the on-disk lockfile as already-trusted)
/// leaves the list empty, making every gate a no-op.
fn install_resolution_verifiers(
    config: &Config,
    trust_lockfile: bool,
    clients: (
        &Arc<pnpm_network::ThrottledClient>,
        &Arc<InMemoryPackageMetaCache>,
        Option<&Arc<super::super::AuthHeaders>>,
    ),
    shared: (
        &pnpm_resolving_resolver_base::PlannedCanonicalFetches,
        Option<&super::super::SharedInstallCaches>,
    ),
) -> Result<Vec<Arc<dyn super::super::ResolutionVerifier>>, InstallError> {
    if trust_lockfile {
        return Ok(Vec::new());
    }
    let (http_client_arc, meta_cache, auth_override) = clients;
    let (planned_canonical_fetches, shared_caches) = shared;
    build_resolution_verifiers(
        config,
        Arc::clone(http_client_arc),
        Some(Arc::clone(meta_cache) as Arc<dyn pnpm_resolving_npm_resolver::PackageMetaCache>),
        auth_override.cloned(),
        None,
        Some(std::sync::Arc::clone(planned_canonical_fetches)),
        shared_caches.map(|caches| caches.verifier_lookups.clone()),
    )
    .map_err(InstallError::BuildVerifiers)
}
