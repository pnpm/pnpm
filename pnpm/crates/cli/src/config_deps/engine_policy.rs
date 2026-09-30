//! The `minimumReleaseAge` and `trustPolicy` gates applied when pnpm resolves
//! a package manager for itself.

use super::{
    Config, EnvInstallerContext, PNPM_VERSION, ResolveOptions, Result, resolve_engine_with,
};
use miette::{IntoDiagnostic, WrapErr};

/// The pnpm version to record for a range pin that `running_version`
/// satisfies.
///
/// The running pnpm was installed without the project's settings, so nothing
/// has held it to the project's `minimumReleaseAge`. Every other contributor
/// switches to the recorded version under that cutoff, so an immature running
/// version gives way to the newest mature version in `range`
/// (pnpm/pnpm#16431). When `range` has no mature version, the running version
/// is kept: no switch could pick a better one.
pub async fn mature_pnpm_version_for_range(
    config: &Config,
    range: &str,
    running_version: &str,
) -> Result<String> {
    if engine_release_cutoff(config)?.is_none() {
        return Ok(running_version.to_string());
    }
    let context = EnvInstallerContext::for_package_manager(config)?;
    let range_opts = resolve_options_without_running_exemption(config)?;
    // The running pnpm is already executing, so only its age is in question.
    let mut running_opts = range_opts.clone();
    running_opts.policy.trust_policy = None;
    for (specifier, opts) in [(running_version, &running_opts), (range, &range_opts)] {
        if let Some(version) = pick_without_violation(&context, specifier, opts).await? {
            return Ok(version);
        }
    }
    Ok(running_version.to_string())
}

/// The install path's resolve options without the maturity exemption they
/// grant the running pnpm, which is the version under question here.
fn resolve_options_without_running_exemption(config: &Config) -> Result<ResolveOptions> {
    let mut opts = engine_resolve_options(config, false)?;
    opts.policy.published_by_exclude = published_by_exclude(config, None)?;
    Ok(opts)
}

async fn pick_without_violation(
    context: &EnvInstallerContext,
    specifier: &str,
    opts: &ResolveOptions,
) -> Result<Option<String>> {
    let resolved = resolve_engine_with(context, "pnpm", specifier, opts).await?;
    Ok(resolved
        .filter(|resolved| resolved.policy_violation.is_none())
        .map(|resolved| resolved.version))
}

fn published_by_exclude_for_engine(
    config: &Config,
) -> Result<Option<pnpm_config::version_policy::PackageVersionPolicy>> {
    published_by_exclude(config, Some(format!("pnpm@{PNPM_VERSION}")))
}

/// The `minimumReleaseAgeExclude` policy, plus `also_exclude`.
fn published_by_exclude(
    config: &Config,
    also_exclude: Option<String>,
) -> Result<Option<pnpm_config::version_policy::PackageVersionPolicy>> {
    let mut exclude_patterns = config.minimum_release_age_exclude.clone().unwrap_or_default();
    exclude_patterns.extend(also_exclude);
    pnpm_config::version_policy::create_package_version_policy(&exclude_patterns)
        .into_diagnostic()
        .wrap_err("compile the minimum-release-age-exclude policy")
        .map(Some)
}

/// The resolve options carrying the maturity and trust policies of the
/// install path. `ignore_maturity` drops `publishedBy` so the lookup
/// returns the registry's real `latest` tag.
pub(super) fn engine_resolve_options(
    config: &Config,
    ignore_maturity: bool,
) -> Result<ResolveOptions> {
    let published_by = if ignore_maturity { None } else { engine_release_cutoff(config)? };
    // The running version is already on this machine, so hiding it behind the
    // maturity cutoff protects nothing — it only makes a dist-tag that points
    // at it fall back to an older release, downgrading the user
    // (pnpm/pnpm#13883).
    let published_by_exclude =
        if ignore_maturity { None } else { published_by_exclude_for_engine(config)? };
    let trust_policy = match config.trust_policy {
        pnpm_config::TrustPolicy::Off => None,
        pnpm_config::TrustPolicy::NoDowngrade => Some(pnpm_config::TrustPolicy::NoDowngrade),
    };
    let trust_policy_exclude = config.trust_policy_exclude
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(pnpm_config::version_policy::create_package_version_policy)
        .transpose()
        .into_diagnostic()
        .wrap_err("compile the trust-policy-exclude policy")?;

    Ok(ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            default_tag: Some("latest".to_string()),
            ..Default::default()
        },
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by,
            published_by_exclude,
            trust_policy,
            trust_policy_exclude,
            trust_policy_ignore_after: config.trust_policy_ignore_after,
            ..Default::default()
        },
        ..ResolveOptions::default()
    })
}

/// Fail closed when the configured maturity cutoff cannot be represented.
fn engine_release_cutoff(config: &Config) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    Ok(match config.resolved_minimum_release_age() {
        Some(minutes) => {
            let minutes = i64::try_from(minutes)
                .into_diagnostic()
                .wrap_err("convert minimumReleaseAge to minutes")?;
            let duration = chrono::Duration::try_minutes(minutes)
                .ok_or_else(|| miette::miette!("minimumReleaseAge is too large"))?;
            Some(
                chrono::Utc::now()
                    .checked_sub_signed(duration)
                    .ok_or_else(|| miette::miette!("minimumReleaseAge cutoff is out of range"))?,
            )
        }
        None => None,
    })
}

#[cfg(test)]
mod tests;
