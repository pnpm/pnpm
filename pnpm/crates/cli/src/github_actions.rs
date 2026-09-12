use pnpm_config::Config;

/// GitHub Actions dependencies are opt-in. Reading them means running `git
/// ls-remote` against every referenced repository, so `pnpm outdated` and
/// `pnpm update` only look at workflow files when asked to, either with
/// `--include-github-actions` or with `update.githubActions: true`.
pub(crate) fn opted_in(include_github_actions: bool, config: &Config) -> bool {
    include_github_actions || config.update_config.github_actions == Some(true)
}

#[cfg(test)]
mod tests;
