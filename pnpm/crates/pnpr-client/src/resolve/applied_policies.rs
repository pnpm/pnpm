use super::{Deserialize, PnprClientError, ResolveProjectsOptions};

/// The policies the server applied to a resolution, echoed in the `done`
/// frame so the client can verify that requested verification and resolution
/// policies were actually enforced by the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the `applied` object of the pnpr `done` frame."
    )
)]
pub struct AppliedPolicies {
    pub auto_install_peers: bool,
    pub dedupe_peers: bool,
    pub exclude_links_from_lockfile: bool,
    pub add_missing_peer_types: bool,
    /// Absent from a server that predates the setting.
    #[serde(default)]
    pub project_scoped_optional_peers: bool,
    pub frozen_lockfile: bool,
    pub resolution_mode: String,
    #[serde(default)]
    pub minimum_release_age: Option<u64>,
    #[serde(default)]
    pub trust_policy: Option<String>,
}

pub(super) fn assert_applied_policies(
    applied: Option<&AppliedPolicies>,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    let Some(applied) = applied else {
        return Ok(());
    };
    assert_verification_policies(applied, opts)?;
    assert_resolution_settings(applied, opts)?;
    assert_reuse_policies(applied, opts)?;
    Ok(())
}

fn assert_verification_policies(
    applied: &AppliedPolicies,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if opts.verification.trust_policy == pnpm_config::TrustPolicy::NoDowngrade
        && applied.trust_policy.as_deref() != Some("no-downgrade")
    {
        return Err(PnprClientError::Protocol(
            "pnpr server ignored requested trustPolicy (expected no-downgrade)".to_string(),
        ));
    }
    if opts.verification.minimum_release_age.is_some()
        && opts.verification.minimum_release_age != applied.minimum_release_age
    {
        return Err(PnprClientError::Protocol(format!(
            "pnpr server ignored requested minimumReleaseAge (expected {:?}, got {:?})",
            opts.verification.minimum_release_age, applied.minimum_release_age,
        )));
    }
    Ok(())
}

fn assert_resolution_settings(
    applied: &AppliedPolicies,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    let resolution = &opts.resolution;
    let flags = [
        ("autoInstallPeers", resolution.auto_install_peers, applied.auto_install_peers),
        ("dedupePeers", resolution.dedupe_peers, applied.dedupe_peers),
        (
            "excludeLinksFromLockfile",
            resolution.exclude_links_from_lockfile,
            applied.exclude_links_from_lockfile,
        ),
        ("addMissingPeerTypes", resolution.add_missing_peer_types, applied.add_missing_peer_types),
        (
            "projectScopedOptionalPeers",
            resolution.project_scoped_optional_peers,
            applied.project_scoped_optional_peers,
        ),
    ];
    if let Some((name, requested, got)) = flags
        .into_iter()
        .find(|(_, requested, got)| requested.is_some_and(|val| val != *got))
    {
        return Err(PnprClientError::Protocol(format!(
            "pnpr server ignored requested {name} (expected {requested:?}, got {got})",
        )));
    }
    let expected_mode = match opts.resolution.resolution_mode {
        pnpm_config::ResolutionMode::Highest => "highest",
        pnpm_config::ResolutionMode::TimeBased => "time-based",
        pnpm_config::ResolutionMode::LowestDirect => "lowest-direct",
    };
    if applied.resolution_mode != expected_mode {
        return Err(PnprClientError::Protocol(format!(
            "pnpr server ignored requested resolutionMode (expected {expected_mode}, got {})",
            applied.resolution_mode,
        )));
    }
    Ok(())
}

fn assert_reuse_policies(
    applied: &AppliedPolicies,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if opts.reuse.frozen_lockfile != applied.frozen_lockfile {
        return Err(PnprClientError::Protocol(format!(
            "pnpr server did not respect frozenLockfile (expected {}, got {})",
            opts.reuse.frozen_lockfile, applied.frozen_lockfile,
        )));
    }
    Ok(())
}
