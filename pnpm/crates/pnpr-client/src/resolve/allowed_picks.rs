use pnpm_lockfile::{
    Prefix,
    VersionPart,
};

use super::{
    Lockfile,
    PnprClientError,
    ResolveProjectsOptions,
};

pub(crate) fn assert_allowed_picks(
    allowed_picks: &[String],
    lockfile: &Lockfile,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if allowed_picks.is_empty() {
        return Ok(());
    }
    assert_loose_release_age_policy(opts)?;
    let exact_versions = exact_lockfile_versions(lockfile);
    for pick in allowed_picks {
        assert_exact_pick(pick, &exact_versions)?;
    }
    Ok(())
}

fn assert_loose_release_age_policy(opts: &ResolveProjectsOptions) -> Result<(), PnprClientError> {
    if opts.verification.minimum_release_age.is_none_or(|age| age == 0)
        || opts.verification.minimum_release_age_strict
    {
        return Err(PnprClientError::Protocol(
            "pnpr returned allowedPicks without an active loose minimumReleaseAge policy"
                .to_string(),
        ));
    }
    Ok(())
}

fn exact_lockfile_versions(lockfile: &Lockfile) -> std::collections::HashSet<String> {
    lockfile.packages
        .as_ref()
        .into_iter()
        .flat_map(|packages| packages.keys())
        .filter_map(|key| {
            if is_pattern_name(&key.name.to_string()) {
                return None;
            }
            if key.suffix.prefix() != Prefix::None {
                return None;
            }
            let version = match key.suffix.version() {
                VersionPart::Semver(version) | VersionPart::RegistryQualified { version, .. } => {
                    version
                }
                VersionPart::File(_) | VersionPart::NonSemver(_) => return None,
            };
            Some(format!("{}@{version}", key.name))
        })
        .collect()
}

fn assert_exact_pick(
    pick: &str,
    exact_versions: &std::collections::HashSet<String>,
) -> Result<(), PnprClientError> {
    let Some((name, versions)) = pick.rsplit_once('@') else {
        return Err(invalid_allowed_pick(pick));
    };
    if name.is_empty() || is_pattern_name(name) || versions.is_empty() {
        return Err(invalid_allowed_pick(pick));
    }
    for version in versions.split(" || ") {
        if version.is_empty() || !exact_versions.contains(&format!("{name}@{version}")) {
            return Err(invalid_allowed_pick(pick));
        }
    }
    Ok(())
}

fn is_pattern_name(name: &str) -> bool {
    name.starts_with('!')
        || name
            .chars()
            .any(|character| matches!(character, '*' | '?'))
}

fn invalid_allowed_pick(pick: &str) -> PnprClientError {
    PnprClientError::Protocol(format!(
        "pnpr returned an allowedPick that is not an exact lockfile version: {pick:?}",
    ))
}
