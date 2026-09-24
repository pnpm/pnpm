use pnpm_config::Config;
use pnpm_github_actions::ReleaseAge;
use pnpm_package_manager::PickPolicy;

/// GitHub Actions dependencies are opt-in. Reading them means running `git
/// ls-remote` against every referenced repository, so `pnpm outdated` and
/// `pnpm update` only look at workflow files when asked to, either with
/// `--include-github-actions` or with `update.githubActions: true`.
pub(crate) fn opted_in(include_github_actions: bool, config: &Config) -> bool {
    include_github_actions || config.update_config.github_actions == Some(true)
}

/// The `minimumReleaseAge` policy GitHub Actions versions are held to: the
/// one registry packages are, with `minimumReleaseAgeExclude` matched against
/// the action and repository names.
pub(crate) fn release_age(config: &Config) -> miette::Result<Option<ReleaseAge>> {
    let policy = PickPolicy::from_config(config).map_err(miette::Report::new)?;
    Ok(policy.published_by.map(|published_by| ReleaseAge {
        published_by,
        exclude: policy.published_by_exclude,
    }))
}

#[cfg(test)]
mod tests;
