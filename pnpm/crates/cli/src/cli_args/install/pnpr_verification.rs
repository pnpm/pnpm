use super::{
    Lockfile,
    PnprLink,
    PnprSession,
    Reporter,
    State,
    build_resolution_verifiers,
};
use pnpm_lockfile_verification::{
    ExoticSubdepsPolicy,
    VerifiedOfflineLockfile,
    VerifyLockfileResolutionsOptions,
    verify_lockfile_resolutions,
};

type VerifierList = Vec<std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>;

/// Offline structural and shape verification for an existing lockfile before
/// delegating policy verification or resolution to the pnpr server. Runs even
/// under `--trust-lockfile`, which opts out only of online supply-chain policy
/// verifiers.
pub(super) fn verify_pnpr_input_lockfile<'a>(
    state: &State,
    link: &PnprLink<'_>,
    lockfile: &'a Lockfile,
) -> miette::Result<VerifiedOfflineLockfile<'a>> {
    let local_policy = pnpm_deps_restorer::local_resolution_policy_for_config(state.config, false);
    let result = if link.lockfile.fix {
        VerifiedOfflineLockfile::verify_for_repair_with_policy(lockfile, Some(local_policy))
    } else {
        VerifiedOfflineLockfile::verify_with_policy(lockfile, Some(local_policy))
    };
    result.map_err(miette::Report::new)
}

/// Verify the settled lockfile returned by the pnpr server (and merged locally
/// when `--fix-lockfile` repaired a partial workspace selection) before writing
/// it to disk, recording it in `lockfile-verified.jsonl`, or linking it.
pub(super) async fn verify_settled_pnpr_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    link: &PnprLink<'_>,
    session: &PnprSession<'_>,
    exotic_subdeps: &ExoticSubdepsPolicy,
    lockfile: &Lockfile,
) -> miette::Result<Option<VerifierList>> {
    let local_policy = pnpm_deps_restorer::enrich_local_policy_for_install(
        pnpm_deps_restorer::local_resolution_policy_for_config(state.config, false),
        lockfile,
        pnpm_deps_restorer::LocalPolicyEnrichDrivers {
            config: Some(state.config),
            http_client: Some(&state.http_client),
            hook: None,
        },
    )
    .await;
    let verified =
        VerifiedOfflineLockfile::verify_with_policy(lockfile, Some(local_policy.clone()))
            .map_err(miette::Report::new)?;
    let settled = SettledPnprLockfile { lockfile, verified, exotic_subdeps, local_policy };
    if link.lockfile.fix && session.partial_selection {
        return verify_merged_repair_policies::<Reporter>(state, link, settled).await;
    }
    if !link.lockfile.trust {
        verify_settled_exotic_subdeps::<Reporter>(state, settled).await?;
    }
    Ok(None)
}

/// The settled lockfile and what its verification needs.
struct SettledPnprLockfile<'a> {
    lockfile: &'a Lockfile,
    verified: VerifiedOfflineLockfile<'a>,
    exotic_subdeps: &'a ExoticSubdepsPolicy,
    local_policy: pnpm_lockfile_verification::LocalResolutionPolicy,
}

async fn verify_settled_exotic_subdeps<Reporter: self::Reporter + 'static>(
    state: &State,
    SettledPnprLockfile {
        lockfile,
        verified,
        exotic_subdeps,
        local_policy,
    }: SettledPnprLockfile<'_>,
) -> miette::Result<()> {
    if !exotic_subdeps.is_enforced()
        || verified.is_cached_by_content(&state.config.cache_dir, &[], exotic_subdeps)
    {
        return Ok(());
    }
    verify_lockfile_resolutions::<Reporter>(
        lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: exotic_subdeps.clone(),
            local_policy: Some(local_policy),
            ..Default::default()
        },
    )
    .await
    .map_err(miette::Report::new)
}

async fn verify_merged_repair_policies<Reporter: self::Reporter + 'static>(
    state: &State,
    link: &PnprLink<'_>,
    SettledPnprLockfile {
        lockfile,
        verified,
        exotic_subdeps,
        local_policy,
    }: SettledPnprLockfile<'_>,
) -> miette::Result<Option<VerifierList>> {
    if link.lockfile.trust {
        return Ok(Some(Vec::new()));
    }
    let verifiers = state_resolution_verifiers(state)?;
    if (!verifiers.is_empty() || exotic_subdeps.is_enforced())
        && !verified.is_cached_by_content(&state.config.cache_dir, &verifiers, exotic_subdeps)
    {
        verify_lockfile_resolutions::<Reporter>(
            lockfile,
            &verifiers,
            &VerifyLockfileResolutionsOptions {
                exotic_subdeps: exotic_subdeps.clone(),
                local_policy: Some(local_policy),
                ..Default::default()
            },
        )
        .await
        .map_err(miette::Report::new)?;
    }
    Ok(Some(verifiers))
}

pub(super) fn state_resolution_verifiers(state: &State) -> miette::Result<VerifierList> {
    build_resolution_verifiers(
        state.config,
        std::sync::Arc::clone(&state.http_client),
        None,
        None,
        None,
        None,
        None,
    )
    .map_err(miette::Report::new)
}
