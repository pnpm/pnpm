//! Interactive selection for `pacquet update --interactive`.
//!
//! Ports the data-gathering half of pnpm's
//! [`interactiveUpdate`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/update/index.ts#L195-L280):
//! find the direct dependencies that have a newer version available
//! (within the current range, or the `latest` tag under `--latest`),
//! show them in a checkbox prompt, and return the names the user picked
//! so the regular update path can run with them as selectors.
//!
//! Unlike pnpm — which fans out through `@pnpm/deps.inspection.outdated`
//! — pacquet computes "outdated" inline: the current version comes from
//! the wanted lockfile and the target from a single packument fetch per
//! dependency. The choice list is intentionally flat (pnpm groups by
//! dependency type); the prompt is a `dialoguer` multi-select.

use dialoguer::MultiSelect;
use miette::{IntoDiagnostic, miette};
use node_semver::Version;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::Package;

/// One outdated direct dependency offered in the prompt.
struct OutdatedChoice {
    name: String,
    current: Version,
    target: Version,
}

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
    let choices =
        collect_outdated(manifest, lockfile, config, http_client, latest, include_direct).await?;

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
        .map(|choice| format!("{} {} ❯ {}", choice.name, choice.current, choice.target))
        .collect();

    let selected_indices = MultiSelect::new()
        .with_prompt("Choose which packages to update (space to select, enter to confirm)")
        .items(&labels)
        .interact()
        .into_diagnostic()
        .map_err(|err| miette!("interactive update selection failed: {err}"))?;

    let selected: Vec<String> =
        selected_indices.into_iter().map(|index| choices[index].name.clone()).collect();

    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(selected))
}

/// Build the outdated-direct-dependency list. A dependency is outdated
/// when its registry target (highest version satisfying the manifest
/// range, or the `latest` tag under `--latest`) is strictly newer than
/// the version currently pinned in the lockfile. Dependencies without a
/// lockfile pin, without a registry target, or whose specifier is not a
/// plain semver range are skipped — they can't be diffed.
async fn collect_outdated(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &ThrottledClient,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Vec<OutdatedChoice>> {
    let current_versions = current_versions_from_lockfile(lockfile, include_direct);

    let mut direct: Vec<(String, String)> = Vec::new();
    for &group in include_direct {
        for (name, range) in manifest.dependencies([group]) {
            direct.push((name.to_string(), range.to_string()));
        }
    }

    let mut choices = Vec::new();
    for (name, range) in direct {
        let Some(current) = current_versions.get(&name).cloned() else { continue };
        let package = match Package::fetch_from_registry(
            &name,
            http_client,
            &config.registry,
            &config.auth_headers,
        )
        .await
        {
            Ok(package) => package,
            // A dependency the registry can't serve (private, renamed,
            // network blip) is simply not offered for update rather than
            // failing the whole prompt.
            Err(_) => continue,
        };

        let target_raw = if latest {
            package.dist_tag("latest").map(ToString::to_string)
        } else {
            package.pinned_version(&range).map(|version| version.serialize(true).replace('^', ""))
        };
        let Some(target_raw) = target_raw else { continue };
        let Ok(target) = target_raw.parse::<Version>() else { continue };

        if target > current {
            choices.push(OutdatedChoice { name, current, target });
        }
    }

    Ok(choices)
}

/// Map each direct dependency name to its lockfile-pinned semver
/// version. Only the root importer is consulted (pacquet's single-project
/// scope); entries whose resolved version isn't a plain semver (`link:`,
/// `file:`, non-semver runtimes) are omitted.
fn current_versions_from_lockfile(
    lockfile: Option<&Lockfile>,
    include_direct: &[DependencyGroup],
) -> std::collections::HashMap<String, Version> {
    let mut map = std::collections::HashMap::new();
    let Some(importer) = lockfile.and_then(Lockfile::root_project) else { return map };
    for (name, spec) in importer.dependencies_by_groups(include_direct.iter().copied()) {
        if let Some(version) = spec.version.ver_peer().and_then(|ver| ver.version_semver()) {
            map.insert(name.to_string(), version.clone());
        }
    }
    map
}
