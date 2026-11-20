use super::{
    InstallFrozenLockfileError,
    LocalLockfileInstall,
    Lockfile,
    LockfileVerificationOverride,
    PnprClient,
    PnprClientError,
    PnprLink,
    PnprSession,
    Reporter,
    State,
    VerifiedOfflineLockfile,
    VerifyLockfileOptions,
    VerifyLockfileResolutionsOptions,
    build_resolution_verifiers,
    record_lockfile_verified,
    verify_lockfile_resolutions,
};

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
    lockfile: &Lockfile,
) -> miette::Result<Option<Vec<std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>>>
{
    let verified = VerifiedOfflineLockfile::verify(lockfile).map_err(miette::Report::new)?;
    if !(link.lockfile.fix && session.partial_selection) {
        return Ok(None);
    }
    let verifiers =
        if link.lockfile.trust { Vec::new() } else { state_resolution_verifiers(state)? };
    if !verifiers.is_empty() && !verified.is_cached_by_content(&state.config.cache_dir, &verifiers)
    {
        verify_lockfile_resolutions::<Reporter>(
            lockfile,
            &verifiers,
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .map_err(miette::Report::new)?;
    }
    Ok(Some(verifiers))
}

/// The verification the frozen install runs concurrently with its fetch.
/// `None` under `--trust-lockfile`, and when this exact lockfile was
/// already verified against the same resolvers.
pub(super) fn local_lockfile_verification<'a>(
    state: &'a State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'a>,
    verified_input: VerifiedOfflineLockfile<'a>,
) -> miette::Result<Option<LockfileVerificationOverride<'a>>> {
    if link.lockfile.trust {
        return Ok(None);
    }
    let verifiers = state_resolution_verifiers(state)?;
    if verified_input.is_cached(&state.config.cache_dir, local.lockfile_path, &verifiers) {
        return Ok(None);
    }
    let verify_opts = local_verify_options(state, pnpr_server, link, local);
    let verify_client = PnprClient::new(pnpr_server);
    let cache_dir = state.config.cache_dir.clone();
    let record_lockfile_path = local.lockfile_path.to_path_buf();
    let lockfile = local.lockfile;
    Ok(Some(Box::pin(async move {
        match verify_client.verify_lockfile(verify_opts).await {
            Ok(()) => {
                record_lockfile_verified(
                    Some(&cache_dir),
                    &record_lockfile_path,
                    lockfile,
                    &verifiers,
                );
                Ok(())
            }
            Err(PnprClientError::Verification(verify_err)) => {
                Err(InstallFrozenLockfileError::LockfileVerification(verify_err))
            }
            Err(err) => {
                Err(InstallFrozenLockfileError::ExternalLockfileVerification(err.to_string()))
            }
        }
    }) as LockfileVerificationOverride<'a>))
}

fn local_verify_options(
    state: &State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> VerifyLockfileOptions {
    VerifyLockfileOptions {
        overrides: local.overrides.cloned(),
        lockfile: local.lockfile.clone(),
        trust_lockfile: link.lockfile.trust,
        routing: pnpm_pnpr_client::RegistryRouting {
            registry: local.resolve_registry.to_owned(),
            registries: state.config.registry_declarations(),
            authorization: state.config.auth_headers.for_url(pnpr_server),
        },
        verification: super::pnpr_request::verification_policy(state.config),
    }
}

fn state_resolution_verifiers(
    state: &State,
) -> miette::Result<Vec<std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>> {
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
