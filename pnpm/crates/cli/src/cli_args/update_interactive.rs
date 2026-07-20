//! Interactive selection for `pacquet update --interactive`.
//!
//! The data-gathering half: find the direct dependencies that have a newer
//! version available (within the current range, or the `latest` tag under
//! `--latest`), show them in a checkbox prompt, and return the names the
//! user picked so the regular update path can run with them as selectors.
//!
//! The outdated set is computed by the shared
//! [`collect_outdated_for_importer`], whose root-importer wrapper also backs
//! `pacquet outdated`. The two
//! callers differ only in
//! the [`TargetVersion`] they compare against: `update` targets the
//! version a bump would move to (the `latest` tag under `--latest`,
//! otherwise the highest in-range version). The choice list is
//! intentionally flat; the prompt is a `dialoguer` multi-select.

use crate::cli_args::{
    outdated::{OutdatedPackage, OutdatedQuery, TargetVersion, collect_outdated_for_importer},
    pipelines::InstallFamilySelection,
};
use dialoguer::MultiSelect;
use miette::{IntoDiagnostic, miette};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashSet;

struct InteractiveUpdateProject<'a> {
    manifest: &'a PackageManifest,
    importer_id: String,
}

/// Gather outdated direct dependencies, prompt the user, and return the
/// selected package names. `Ok(None)` means "nothing to do" — either no
/// dependency has an update available or the prompt was answered with an
/// empty selection — and the caller should not run an update.
pub async fn select_packages(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    config: &Config,
    http_client: &ThrottledClient,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Option<Vec<String>>> {
    let projects = [InteractiveUpdateProject { manifest, importer_id: importer_id.to_string() }];
    let choices =
        collect_choices(&projects, lockfile, config, http_client, latest, include_direct).await?;
    prompt_for_packages(&choices, latest)
}

pub(crate) async fn select_packages_for_projects(
    selection: &InstallFamilySelection,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &ThrottledClient,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Option<Vec<String>>> {
    let projects = selection
        .projects
        .iter()
        .filter(|project| selection.selected_dirs.contains(&project.root_dir))
        .map(|project| InteractiveUpdateProject {
            manifest: &project.manifest,
            importer_id: pacquet_workspace::importer_id_from_root_dir(
                &selection.workspace_root,
                &project.root_dir,
            ),
        })
        .collect::<Vec<_>>();
    let choices =
        collect_choices(&projects, lockfile, config, http_client, latest, include_direct).await?;
    prompt_for_packages(&choices, latest)
}

async fn collect_choices(
    projects: &[InteractiveUpdateProject<'_>],
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &ThrottledClient,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Vec<OutdatedPackage>> {
    let target_version = if latest { TargetVersion::Latest } else { TargetVersion::WithinRange };
    let query = OutdatedQuery {
        target_version,
        include_direct,
        match_names: None,
        include_deprecated: false,
    };
    let choices = futures_util::future::join_all(projects.iter().map(|project| {
        collect_outdated_for_importer(
            project.manifest,
            lockfile,
            &project.importer_id,
            config,
            http_client,
            &query,
        )
    }))
    .await;
    let mut unique = HashSet::new();
    let mut collected = Vec::new();
    for choices in choices {
        for choice in choices? {
            let key = (
                choice.alias.clone(),
                choice.package_name.clone(),
                choice.current.to_string(),
                choice.target.to_string(),
            );
            if unique.insert(key) {
                collected.push(choice);
            }
        }
    }
    Ok(collected)
}

fn prompt_for_packages(
    choices: &[OutdatedPackage],
    latest: bool,
) -> miette::Result<Option<Vec<String>>> {
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

#[cfg(test)]
mod tests;
