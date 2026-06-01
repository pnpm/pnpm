//! Interactive selection for `pacquet update --interactive`.
//!
//! Ports the data-gathering half of pnpm's
//! [`interactiveUpdate`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/update/index.ts#L195-L280):
//! find the direct dependencies that have a newer version available
//! (within the current range, or the `latest` tag under `--latest`),
//! show them in a checkbox prompt, and return the names the user picked
//! so the regular update path can run with them as selectors.
//!
//! The outdated set is computed by the shared
//! [`collect_outdated`], which also backs `pacquet outdated`. The two
//! callers differ only in
//! the [`TargetVersion`] they compare against: `update` targets the
//! version a bump would move to (the `latest` tag under `--latest`,
//! otherwise the highest in-range version). The choice list is
//! intentionally flat (pnpm groups by dependency type); the prompt is a
//! `dialoguer` multi-select.

use crate::cli_args::outdated::{OutdatedQuery, TargetVersion, collect_outdated};
use dialoguer::MultiSelect;
use miette::{IntoDiagnostic, miette};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};

/// Gather outdated direct dependencies, prompt the user, and return the
/// selected package names. `Ok(None)` means "nothing to do" — either no
/// dependency has an update available or the prompt was answered with an
/// empty selection — and the caller should not run an update.
pub async fn select_packages(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &ThrottledClient,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Option<Vec<String>>> {
    let target_version = if latest { TargetVersion::Latest } else { TargetVersion::WithinRange };
    let query = OutdatedQuery {
        target_version,
        include_direct,
        match_names: None,
        include_deprecated: false,
    };
    let choices = collect_outdated(manifest, lockfile, config, http_client, &query).await?;

    if choices.is_empty() {
        let message = if latest {
            "All of your dependencies are already up to date"
        } else {
            "All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json"
        };
        println!("{message}");
        return Ok(None);
    }

    let labels: Vec<String> = choices
        .iter()
        .map(|choice| format!("{} {} ❯ {}", choice.alias, choice.current, choice.target))
        .collect();

    let selected_indices = MultiSelect::new()
        .with_prompt("Choose which packages to update (space to select, enter to confirm)")
        .items(&labels)
        .interact()
        .into_diagnostic()
        .map_err(|err| miette!("interactive update selection failed: {err}"))?;

    let selected: Vec<String> =
        selected_indices.into_iter().map(|index| choices[index].alias.clone()).collect();

    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(selected))
}
