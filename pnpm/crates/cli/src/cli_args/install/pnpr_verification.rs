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
    link: &PnprLink<'_>,
    lockfile: &'a Lockfile,
) -> miette::Result<VerifiedOfflineLockfile<'a>> {
    let result = if link.lockfile.fix {
        VerifiedOfflineLockfile::verify_for_repair(lockfile)
    } else {
        VerifiedOfflineLockfile::verify(lockfile)
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
    let verified = VerifiedOfflineLockfile::verify(lockfile).map_err(miette::Report::new)?;
    if link.lockfile.fix && session.partial_selection {
        return verify_merged_repair_policies::<Reporter>(
            state,
            link,
            verified,
            exotic_subdeps,
            lockfile,
        )
        .await;
    }
    if !link.lockfile.trust {
        verify_settled_exotic_subdeps::<Reporter>(state, verified, exotic_subdeps, lockfile).await?;
    }
    Ok(None)
}

async fn verify_settled_exotic_subdeps<Reporter: self::Reporter + 'static>(
    state: &State,
    verified: VerifiedOfflineLockfile<'_>,
    exotic_subdeps: &ExoticSubdepsPolicy,
    lockfile: &Lockfile,
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
            ..Default::default()
        },
    )
    .await
    .map_err(miette::Report::new)
}

async fn verify_merged_repair_policies<Reporter: self::Reporter + 'static>(
    state: &State,
    link: &PnprLink<'_>,
    verified: VerifiedOfflineLockfile<'_>,
    exotic_subdeps: &ExoticSubdepsPolicy,
    lockfile: &Lockfile,
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
