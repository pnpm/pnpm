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
    pub project_scoped_optional_peers: bool,
    pub frozen_lockfile: bool,
    pub resolution_mode: String,
    pub trust_lockfile: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Vec<String>,
    pub minimum_release_age_ignore_missing_time: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub trust_policy: Option<String>,
    pub trust_policy_exclude: Vec<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub trust_policy_ignore_after: Option<u64>,
}

pub(super) fn assert_applied_policies(
    applied: Option<&AppliedPolicies>,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    let Some(applied) = applied else {
        return Err(PnprClientError::Protocol("pnpr server omitted appliedPolicies".to_string()));
    };
    assert_verification_policies(applied, &opts.verification, opts.reuse.trust_lockfile)?;
    assert_resolution_settings(applied, opts)?;
    assert_reuse_policies(applied, opts)?;
    Ok(())
}

pub(crate) fn assert_verification_policies(
    applied: &AppliedPolicies,
    policy: &crate::VerificationPolicy,
    trust_lockfile: bool,
) -> Result<(), PnprClientError> {
    let expected_trust = match policy.trust_policy {
        pnpm_config::TrustPolicy::Off => None,
        pnpm_config::TrustPolicy::NoDowngrade => Some("no-downgrade"),
    };
    let checks = [
        ("trustPolicy", applied.trust_policy.as_deref() == expected_trust),
        ("minimumReleaseAge", applied.minimum_release_age == policy.minimum_release_age),
        (
            "minimumReleaseAgeExclude",
            applied.minimum_release_age_exclude.as_slice()
                == policy.minimum_release_age_exclude.as_deref().unwrap_or_default(),
        ),
        (
            "minimumReleaseAgeIgnoreMissingTime",
            applied.minimum_release_age_ignore_missing_time
                == policy.minimum_release_age_ignore_missing_time,
        ),
        (
            "trustPolicyExclude",
            applied.trust_policy_exclude.as_slice()
                == policy.trust_policy_exclude.as_deref().unwrap_or_default(),
        ),
        (
            "trustPolicyIgnoreAfter",
            applied.trust_policy_ignore_after == policy.trust_policy_ignore_after,
        ),
        ("trustLockfile", applied.trust_lockfile == trust_lockfile),
    ];
    if let Some((name, _)) = checks.into_iter().find(|(_, matches)| !matches) {
        return Err(PnprClientError::Protocol(format!("pnpr server ignored requested {name}")));
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
