use crate::cli_args::{
    catalogs::configured_catalogs, global::GlobalError, listed_projects::listed_projects,
    recursive::selectors_narrow_the_run,
};
use clap::{Args, CommandFactory};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    IssuesByProjects, check_peer_dependencies_from_lockfile, filter_peer_issues, render_peer_issues,
};
use pnpm_global::{GlobalPackageInfo, scan_global_packages};
use pnpm_lockfile::{Lockfile, WantedLockfileSelection};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct PeersArgs {
    /// Check each globally installed package group separately.
    #[clap(short = 'g', long)]
    pub global: bool,

    #[clap(long)]
    pub json: bool,

    #[clap(long)]
    pub lockfile_only: bool,

    /// Subcommand and arguments. The only subcommand is `check`, which is
    /// also what a bare `pnpm peers` runs.
    pub params: Vec<String>,
}

/// The outcome of `peers`, which the CLI harness maps to a process exit
/// code: `0` for [`PeersOutcome::NoIssues`] and `1` for the other two,
/// matching pnpm. Returning the outcome (rather than terminating here)
/// keeps [`PeersArgs::run`] composable and process termination in one
/// place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeersOutcome {
    NoIssues,
    IssuesFound,
    UnknownSubcommand,
}

impl PeersArgs {
    pub fn run(
        self,
        config: &Config,
        dir: &std::path::Path,
        recursive: bool,
        recursive_from_command_line: bool,
    ) -> miette::Result<PeersOutcome> {
        if !matches!(self.params.first().map(String::as_str), Some("check") | None) {
            print_peers_help();
            return Ok(PeersOutcome::UnknownSubcommand);
        }

        let (issues, undeclared) = if self.global {
            (self.global_issues(config)?, UndeclaredWarnings::new())
        } else {
            let recursive =
                recursive && !dedicated_subproject(config, dir, recursive_from_command_line);
            self.workspace_issues(config, dir, recursive)?
        };
        self.print_issues(&issues, &undeclared)
    }

    fn workspace_issues(
        &self,
        config: &Config,
        dir: &Path,
        recursive: bool,
    ) -> miette::Result<(IssuesByProjects, UndeclaredWarnings)> {
        let projects = listed_projects(config, dir, recursive)?;
        if config.shares_one_lockfile() || config.workspace_dir.is_none() {
            self.shared_lockfile_issues(config, dir, projects)
        } else {
            self.dedicated_lockfile_issues(config, dir, projects, recursive)
        }
    }

    fn shared_lockfile_issues(
        &self,
        config: &Config,
        dir: &Path,
        projects: Vec<(PathBuf, Option<String>)>,
    ) -> miette::Result<(IssuesByProjects, UndeclaredWarnings)> {
        let lockfile_dir = config.lockfile_dir_for(dir);
        let project_dirs: Vec<PathBuf> = projects
            .into_iter()
            .map(|(project_dir, _)| project_dir)
            .collect();
        let lockfile = self.load_lockfile(
            &config.install_state_dir,
            lockfile_dir,
            &config.wanted_lockfile_selection(),
        )?;
        let issues = checked_issues(config, lockfile_dir, &project_dirs, lockfile.as_ref())?;
        let undeclared =
            undeclared_warnings(config, lockfile_dir, &project_dirs, lockfile.as_ref());
        Ok((issues, undeclared))
    }

    fn dedicated_lockfile_issues(
        &self,
        config: &Config,
        dir: &Path,
        projects: Vec<(PathBuf, Option<String>)>,
        recursive: bool,
    ) -> miette::Result<(IssuesByProjects, UndeclaredWarnings)> {
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        let mut issues = IssuesByProjects::new();
        let mut undeclared = UndeclaredWarnings::new();
        for (project_dir, name) in projects {
            let project_key = if recursive {
                pnpm_workspace::importer_id_from_root_dir(workspace_root, &project_dir)
            } else {
                ".".to_string()
            };
            let (mut project_issues, mut project_undeclared) =
                self.dedicated_project_issues(config, &project_dir, name.as_deref())?;
            if let Some(entry) = project_issues.remove(".") {
                issues.insert(project_key.clone(), entry);
            }
            if let Some(peers) = project_undeclared.remove(".") {
                undeclared.insert(project_key, peers);
            }
        }
        Ok((issues, undeclared))
    }

    fn dedicated_project_issues(
        &self,
        config: &Config,
        project_dir: &Path,
        name: Option<&str>,
    ) -> miette::Result<(IssuesByProjects, UndeclaredWarnings)> {
        let mut project_config = config.clone();
        project_config.anchor_dedicated_project(project_dir, name);
        project_config.resolve_peers_from_workspace_root = false;
        let lockfile = self.load_lockfile(
            &project_config.install_state_dir,
            project_dir,
            &project_config.wanted_lockfile_selection(),
        )?;
        let project_dirs = [project_dir.to_path_buf()];
        let issues =
            checked_issues(&project_config, project_dir, &project_dirs, lockfile.as_ref())?;
        let undeclared =
            undeclared_warnings(&project_config, project_dir, &project_dirs, lockfile.as_ref());
        Ok((issues, undeclared))
    }

    fn global_issues(&self, config: &Config) -> miette::Result<IssuesByProjects> {
        let global_dir =
            config.global_pkg_dir.as_deref().ok_or(GlobalError::MissingGlobalPackageDir)?;
        let groups = scan_global_packages(global_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("read global packages in {}", global_dir.display()))?;
        // Each group installs without catalogs and as its own workspace root
        // (see `global_group_config`), so the home's catalogs must not
        // resolve its peer ranges either.
        let mut group_config = config.clone();
        group_config.catalogs = None;
        group_config.workspace_dir = None;
        let default_selection = WantedLockfileSelection::default();
        let mut checked_dirs = BTreeSet::new();
        let mut issues = IssuesByProjects::new();
        for group in groups {
            if !checked_dirs.insert(group.install_dir.clone()) {
                continue;
            }
            let install_state_dir = group.install_dir.join("node_modules").join(".pnpm");
            let lockfile =
                self.load_lockfile(&install_state_dir, &group.install_dir, &default_selection)?;
            let mut group_issues = checked_issues(
                &group_config,
                &group.install_dir,
                std::slice::from_ref(&group.install_dir),
                lockfile.as_ref(),
            )?;
            if let Some(root_issues) = group_issues.remove(".") {
                issues.insert(global_group_key(&group, &issues), root_issues);
            }
        }
        Ok(issues)
    }

    fn print_issues(
        &self,
        issues: &IssuesByProjects,
        undeclared: &UndeclaredWarnings,
    ) -> miette::Result<PeersOutcome> {
        let no_issues = issues
            .values()
            .all(|pi| pi.bad.is_empty() && pi.missing.is_empty());

        if self.json {
            print_json_issues(issues, undeclared)?;
        } else {
            print_text_issues(issues, undeclared, no_issues);
        }

        Ok(if no_issues { PeersOutcome::NoIssues } else { PeersOutcome::IssuesFound })
    }
    /// The lockfile the check reads: the materialized current lockfile
    /// when there is one, and the wanted lockfile otherwise.
    /// `--lockfile-only` reads the wanted lockfile outright.
    fn load_lockfile(
        &self,
        install_state_dir: &Path,
        lockfile_dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> miette::Result<Option<Lockfile>> {
        let current = if self.lockfile_only {
            None
        } else {
            Lockfile::load_current_from_install_state_dir(install_state_dir)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("load current lockfile in {}", install_state_dir.display())
                })?
        };
        match current {
            Some(lockfile) => Ok(Some(lockfile)),
            None => Lockfile::load_wanted(lockfile_dir, selection)
                .into_diagnostic()
                .wrap_err_with(|| format!("load lockfile in {}", lockfile_dir.display())),
        }
    }
}

fn global_group_key(group: &GlobalPackageInfo, existing: &IssuesByProjects) -> String {
    let mut aliases = group.aliases();
    aliases.sort();
    let key = aliases.join(", ");
    if existing.contains_key(&key) { format!("{key} ({})", group.hash) } else { key }
}

fn print_peers_help() {
    let mut cmd = crate::cli_args::CliArgs::command();
    cmd.build();
    let _ = cmd
        .find_subcommand_mut("peers")
        .expect("peers subcommand")
        .print_help();
}

/// Without `-r` or a workspace selector, `pnpm peers` run inside a subproject
/// of a `sharedWorkspaceLockfile: false` workspace checks only that project's
/// dedicated lockfile.
fn dedicated_subproject(config: &Config, dir: &Path, recursive_from_command_line: bool) -> bool {
    !config.shares_one_lockfile()
        && !recursive_from_command_line
        && !selectors_narrow_the_run(config)
        && config.workspace_dir
            .as_deref()
            .is_some_and(|workspace_dir| {
                pnpm_fs::lexical_normalize(dir) != pnpm_fs::lexical_normalize(workspace_dir)
            })
}

fn checked_issues(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
    lockfile: Option<&Lockfile>,
) -> miette::Result<IssuesByProjects> {
    let catalogs = configured_catalogs(config)?;
    let catalogs =
        (config.workspace_dir.is_some() || config.catalogs.is_some()).then_some(&catalogs);
    let issues = match lockfile {
        Some(lockfile) => check_peer_dependencies_from_lockfile(
            lockfile,
            lockfile_dir,
            config.workspace_dir.as_deref(),
            project_dirs,
            catalogs,
            config.resolve_peers_from_workspace_root,
        )?,
        None => IssuesByProjects::new(),
    };
    Ok(filter_peer_issues(issues, &config.peer_dependency_rules))
}

type UndeclaredWarnings =
    std::collections::BTreeMap<String, Vec<pnpm_deps_inspection_peers::UndeclaredPeer>>;

fn undeclared_warnings(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
    lockfile: Option<&Lockfile>,
) -> UndeclaredWarnings {
    if config.auto_install_peers || !config.dedupe_peer_dependents {
        return UndeclaredWarnings::new();
    }
    let Some(lockfile) = lockfile else { return UndeclaredWarnings::new() };
    let mut inspector = pnpm_deps_inspection_peers::UndeclaredPeerInspector::with_workspace_dir(
        lockfile,
        lockfile_dir,
        config.workspace_dir.as_deref(),
    );
    project_dirs
        .iter()
        .filter_map(|dir| {
            let importer = pnpm_workspace::importer_id_from_root_dir(lockfile_dir, dir);
            let peers = inspector.check(&importer, config.resolve_peers_from_workspace_root.into());
            (!peers.is_empty()).then_some((importer, peers))
        })
        .collect()
}

fn print_undeclared_warnings(warnings: &UndeclaredWarnings) {
    for (project, peers) in warnings {
        for peer in peers {
            pnpm_diagnostics::println!(
                "WARN {}: required peer {}@{} of {} is resolved but is not declared by this project or provided by its dependency ancestors. Declare the peer directly if this project's scripts use it.",
                pnpm_text_sanitize::sanitize_inline(project),
                pnpm_text_sanitize::sanitize_inline(&peer.peer),
                pnpm_text_sanitize::sanitize_inline(&peer.found_version),
                pnpm_text_sanitize::sanitize_inline(&peer.package),
            );
        }
    }
}

fn print_json_issues(
    issues: &IssuesByProjects,
    undeclared: &UndeclaredWarnings,
) -> miette::Result<()> {
    let mut report = serde_json::to_value(issues).into_diagnostic()?;
    for (project, peers) in undeclared {
        report[project]["undeclared"] = serde_json::to_value(peers).into_diagnostic()?;
    }
    let output = serde_json::to_string_pretty(&report)
        .into_diagnostic()
        .wrap_err("serialize issues to JSON")?;
    pnpm_diagnostics::println!("{output}");
    Ok(())
}

fn print_text_issues(issues: &IssuesByProjects, undeclared: &UndeclaredWarnings, no_issues: bool) {
    match (no_issues, undeclared.is_empty()) {
        (true, true) => pnpm_diagnostics::println!("No peer dependency issues found"),
        (true, false) => pnpm_diagnostics::println!(
            "Peer dependencies are satisfied. Declaration warnings follow.",
        ),
        (false, _) => {
            pnpm_diagnostics::println!("Issues with peer dependencies found\n");
            pnpm_diagnostics::println!("{}", render_peer_issues(issues));
        }
    }
    print_undeclared_warnings(undeclared);
}
