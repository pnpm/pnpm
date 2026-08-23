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
//! otherwise the highest in-range version). [`choices::update_choices`]
//! turns that set into the grouped, column-aligned list the prompt
//! renders. Which prompt that is comes in as an [`UpdatePrompt`].

use crate::{
    cli_args::{
        outdated::{
            OutdatedPackage, OutdatedQuery, OutdatedRun, TargetVersion,
            collect_outdated_for_importer, collect_outdated_for_importer_in_run,
            ignored_dependencies_matcher,
        },
        pipelines::InstallFamilySelection,
        sanitize::sanitize_inline,
    },
    github_actions,
};
use dialoguer::MultiSelect;
use miette::{IntoDiagnostic, miette};
use owo_colors::{OwoColorize, Stream};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use std::{collections::HashSet, path::Path, sync::Arc};

struct InteractiveUpdateProject<'a> {
    manifest: &'a PackageManifest,
    importer_id: String,
}

pub(crate) struct InteractiveUpdateOptions<'a> {
    pub latest: bool,
    pub include_direct: &'a [DependencyGroup],
    pub include_github_actions: bool,
    pub prompt: UpdatePrompt,
}

/// One row of the checkbox prompt: the line the user reads, and the
/// package checking it updates. A group heading and a group's header row
/// update nothing — `dialoguer` cannot mark an item unselectable the way
/// pnpm's prompt does, so [`selected_packages`] drops them from the
/// answer instead.
struct PromptRow {
    label: String,
    value: Option<String>,
}

/// How `update --interactive` asks which dependencies to update.
///
/// `clap` never sets it. It exists so a test can answer the prompt the
/// way the upstream suite answers its own — by mocking
/// `@inquirer/prompts` — which `dialoguer` cannot offer, since it reads a
/// terminal.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum UpdatePrompt {
    #[default]
    Dialoguer,
    #[cfg(test)]
    Scripted,
}

impl UpdatePrompt {
    /// The rows the user checked, by index.
    fn select(self, message: &str, rows: &[PromptRow]) -> miette::Result<Vec<usize>> {
        match self {
            Self::Dialoguer => {
                let labels = rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>();
                MultiSelect::new()
                    .with_prompt(message)
                    .items(&labels)
                    .interact()
                    .into_diagnostic()
                    .map_err(|err| miette!("interactive update selection failed: {err}"))
            }
            #[cfg(test)]
            Self::Scripted => Ok(tests::answer_prompt(message, rows)),
        }
    }
}

pub(crate) async fn select_global_package_groups(
    base_config: &'static Config,
    packages: &[String],
    latest: bool,
    prompt: UpdatePrompt,
) -> miette::Result<Option<HashSet<String>>> {
    let global_pkg_dir = base_config.global_pkg_dir.clone().ok_or_else(|| {
        miette!(code = "ERR_PNPM_NO_GLOBAL_BIN_DIR", "Unable to find the global packages directory")
    })?;
    let mut config = base_config.clone();
    config.workspace_dir = None;
    config.shared_workspace_lockfile = false;
    config.lockfile_dir = None;
    // A group's lockfile is written unconditionally (`run_group_install`
    // forces it) because it is where the installed versions are recorded, so
    // reading it back must not depend on the caller's `lockfile` setting.
    config.lockfile = true;
    let config = Config::leak(config);
    let ignored = ignored_dependencies_matcher(config);
    let query = OutdatedQuery {
        target_version: if latest { TargetVersion::Latest } else { TargetVersion::WithinRange },
        include_direct: &[DependencyGroup::Prod],
        match_names: None,
        ignore_names: ignored.as_ref(),
        include_deprecated: false,
    };
    let mut rows: Vec<PromptRow> = Vec::new();
    let global_packages = pnpm_global::scan_global_packages(&global_pkg_dir)
        .map_err(|err| miette!("failed to scan global packages: {err}"))?;
    if global_packages.is_empty() {
        println!("No global packages found");
        return Ok(None);
    }
    // A global group is always updated as a whole, so the params select groups
    // rather than dependencies, the same way `handle_global_update` reads them.
    let matched_packages = if packages.is_empty() {
        global_packages
    } else {
        let matched = global_packages
            .into_iter()
            .filter(|pkg| packages.iter().any(|param| pkg.has_alias(param)))
            .collect::<Vec<_>>();
        if matched.is_empty() {
            println!("No matching global packages found");
            return Ok(None);
        }
        matched
    };
    for pkg in matched_packages {
        let state = crate::State::init(pkg.install_dir.join("package.json"), config, false)
            .map_err(|err| miette::Report::new(err).wrap_err("initialize global state"))?;
        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let outdated = collect_outdated_for_importer(
            &state.manifest,
            lockfile,
            pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY,
            config,
            &state.http_client,
            &query,
        )
        .await?;
        if outdated.is_empty() {
            continue;
        }
        rows.push(PromptRow {
            label: outdated
                .iter()
                .map(|package| {
                    format!(
                        "{} {} → {}",
                        sanitize_inline(&package.alias),
                        package.current,
                        package.target,
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            value: Some(pkg.hash),
        });
    }
    if rows.is_empty() {
        let message = if latest {
            "All of your dependencies are already up to date"
        } else {
            "All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json"
        };
        println!("{message}");
        return Ok(None);
    }
    let selected_indices = prompt.select(
        "Choose which global package groups to update (space to select, enter to confirm)",
        &rows,
    )?;
    let selected = selected_indices
        .into_iter()
        .filter_map(|index| rows.get(index).and_then(|row| row.value.clone()))
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(selected))
}

/// Gather outdated direct dependencies, prompt the user, and return the
/// selected package names. `Ok(None)` means "nothing to do" — either no
/// dependency has an update available or the prompt was answered with an
/// empty selection — and the caller should not run an update.
pub(crate) async fn select_packages<Reporter: self::Reporter>(
    root: &Path,
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    options: InteractiveUpdateOptions<'_>,
) -> miette::Result<Option<Vec<String>>> {
    let projects = [InteractiveUpdateProject { manifest, importer_id: importer_id.to_string() }];
    let mut choices = collect_choices(
        &projects,
        lockfile,
        config,
        http_client,
        options.latest,
        options.include_direct,
    )
    .await?;
    if options.include_github_actions {
        append_github_actions::<Reporter>(
            &mut choices,
            root,
            options.latest,
            config.update_config.github_actions_server.as_deref(),
        )
        .await?;
    }
    prompt_for_packages(&choices, options.latest, config.workspace_dir.is_some(), options.prompt)
}

pub(crate) async fn select_packages_for_projects<Reporter: self::Reporter>(
    root: &Path,
    selection: &InstallFamilySelection,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    options: InteractiveUpdateOptions<'_>,
) -> miette::Result<Option<Vec<String>>> {
    let projects = selection
        .projects
        .iter()
        .filter(|project| selection.selected_dirs.contains(&project.root_dir))
        .map(|project| InteractiveUpdateProject {
            manifest: &project.manifest,
            importer_id: pnpm_workspace::importer_id_from_root_dir(
                &selection.workspace_root,
                &project.root_dir,
            ),
        })
        .collect::<Vec<_>>();
    let mut choices = collect_choices(
        &projects,
        lockfile,
        config,
        http_client,
        options.latest,
        options.include_direct,
    )
    .await?;
    if options.include_github_actions {
        append_github_actions::<Reporter>(
            &mut choices,
            root,
            options.latest,
            config.update_config.github_actions_server.as_deref(),
        )
        .await?;
    }
    prompt_for_packages(&choices, options.latest, true, options.prompt)
}

async fn append_github_actions<Reporter: self::Reporter>(
    choices: &mut Vec<OutdatedPackage>,
    root: &Path,
    latest: bool,
    server_url: Option<&str>,
) -> miette::Result<()> {
    choices.extend(
        github_actions::find_outdated::<Reporter>(root, !latest, None, server_url)
            .await?
            .into_iter()
            .map(OutdatedPackage::from),
    );
    Ok(())
}

async fn collect_choices(
    projects: &[InteractiveUpdateProject<'_>],
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    latest: bool,
    include_direct: &[DependencyGroup],
) -> miette::Result<Vec<OutdatedPackage>> {
    let target_version = if latest { TargetVersion::Latest } else { TargetVersion::WithinRange };
    let ignored = ignored_dependencies_matcher(config);
    let query = OutdatedQuery {
        target_version,
        include_direct,
        match_names: None,
        ignore_names: ignored.as_ref(),
        include_deprecated: false,
    };
    let run = OutdatedRun::new(config, Arc::clone(http_client))?;
    let choices = futures_util::future::join_all(projects.iter().map(|project| {
        collect_outdated_for_importer_in_run(
            project.manifest,
            lockfile,
            &project.importer_id,
            &query,
            &run,
        )
    }))
    .await;
    // Keyed by workspace as well, so an entry each project contributed
    // survives to [`choices::update_choices`] — that is what lets a
    // collapsed row name every project it covers instead of the first.
    let mut unique = HashSet::new();
    let mut collected = Vec::new();
    for choices in choices {
        for choice in choices? {
            let key = (
                choice.alias.clone(),
                choice.package_name.clone(),
                choice.current.to_string(),
                choice.target.to_string(),
                choice.workspace.clone(),
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
    workspaces_enabled: bool,
    prompt: UpdatePrompt,
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

    let groups = choices::update_choices(&choices.iter().collect::<Vec<_>>(), workspaces_enabled);
    let rows = flatten_groups(&groups);

    let selected_indices = prompt
        .select("Choose which dependencies to update (space to select, enter to confirm)", &rows)?;

    let selected = selected_packages(&rows, &selected_indices);
    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(selected))
}

/// Flatten the groups into the one item list the prompt takes.
fn flatten_groups(groups: &[choices::ChoiceGroup]) -> Vec<PromptRow> {
    let mut rows = Vec::new();
    for group in groups {
        rows.push(PromptRow { label: bold(&group.message), value: None });
        rows.extend(
            group
                .rows
                .iter()
                .map(|row| PromptRow { label: row.label.clone(), value: row.value.clone() }),
        );
    }
    rows
}

/// The packages behind `indices`, in the order the user checked them and
/// without repeats — the same package can be offered by two importers.
fn selected_packages(rows: &[PromptRow], indices: &[usize]) -> Vec<String> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for &index in indices {
        let Some(value) = rows.get(index).and_then(|row| row.value.as_ref()) else { continue };
        if seen.insert(value.as_str()) {
            selected.push(value.clone());
        }
    }
    selected
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

mod choices;

#[cfg(test)]
mod tests;
