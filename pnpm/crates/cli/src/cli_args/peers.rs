use crate::cli_args::{
    catalogs::configured_catalogs,
    global::GlobalError,
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};
use clap::{Args, CommandFactory};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    IssuesByProjects, check_peer_dependencies_from_lockfile, filter_peer_issues, render_peer_issues,
};
use pnpm_global::scan_global_packages;
use pnpm_lockfile::Lockfile;
use std::{collections::BTreeSet, path::Path};

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
    ) -> miette::Result<PeersOutcome> {
        if !matches!(self.params.first().map(String::as_str), Some("check") | None) {
            print_peers_help();
            return Ok(PeersOutcome::UnknownSubcommand);
        }

        let issues = if self.global {
            self.global_issues(config)?
        } else {
            let lockfile_dir = config.lockfile_dir_for(dir);
            let project_dirs = checked_project_dirs(config, dir, recursive)?;
            let lockfile = self.load_lockfile(&config.install_state_dir, lockfile_dir)?;
            checked_issues(config, lockfile_dir, &project_dirs, lockfile.as_ref())?
        };
        self.print_issues(&issues)
    }

    fn global_issues(&self, config: &Config) -> miette::Result<IssuesByProjects> {
        let global_dir =
            config.global_pkg_dir.as_deref().ok_or(GlobalError::MissingGlobalPackageDir)?;
        let groups = scan_global_packages(global_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("read global packages in {}", global_dir.display()))?;
        let mut checked_dirs = BTreeSet::new();
        let mut issues = IssuesByProjects::new();
        for group in groups {
            if !checked_dirs.insert(group.install_dir.clone()) {
                continue;
            }
            let install_state_dir = group.install_dir.join("node_modules").join(".pnpm");
            let lockfile = self.load_lockfile(&install_state_dir, &group.install_dir)?;
            let mut group_issues = checked_issues(
                config,
                &group.install_dir,
                std::slice::from_ref(&group.install_dir),
                lockfile.as_ref(),
            )?;
            if let Some(root_issues) = group_issues.remove(".") {
                let mut aliases = group.aliases();
                aliases.sort();
                issues.insert(aliases.join(", "), root_issues);
            }
        }
        Ok(issues)
    }

    fn print_issues(&self, issues: &IssuesByProjects) -> miette::Result<PeersOutcome> {
        let no_issues = issues
            .values()
            .all(|pi| pi.bad.is_empty() && pi.missing.is_empty());

        if self.json {
            let output = serde_json::to_string_pretty(issues)
                .into_diagnostic()
                .wrap_err("serialize issues to JSON")?;
            pnpm_diagnostics::println!("{output}");
        } else if no_issues {
            pnpm_diagnostics::println!("No peer dependency issues found");
        } else {
            pnpm_diagnostics::println!("Issues with peer dependencies found\n");
            pnpm_diagnostics::println!("{}", render_peer_issues(issues));
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
            None => Lockfile::load_wanted_from_dir(lockfile_dir)
                .into_diagnostic()
                .wrap_err_with(|| format!("load lockfile in {}", lockfile_dir.display())),
        }
    }
}

fn print_peers_help() {
    let mut cmd = crate::cli_args::CliArgs::command();
    cmd.build();
    let _ = cmd
        .find_subcommand_mut("peers")
        .expect("peers subcommand")
        .print_help();
}

/// The projects the check covers: the `--filter` selection under
/// `--recursive`, the current directory otherwise.
fn checked_project_dirs(
    config: &Config,
    dir: &std::path::Path,
    recursive: bool,
) -> miette::Result<Vec<std::path::PathBuf>> {
    if !recursive {
        return Ok(vec![dir.to_path_buf()]);
    }
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
    let (projects, _) = discover_workspace_projects(workspace_root, config)?;
    Ok(select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?
        .selected
        .keys()
        .cloned()
        .collect())
}

fn checked_issues(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[std::path::PathBuf],
    lockfile: Option<&Lockfile>,
) -> miette::Result<IssuesByProjects> {
    let catalogs = configured_catalogs(config)?;
    let catalogs =
        (config.workspace_dir.is_some() || config.catalogs.is_some()).then_some(&catalogs);
    let issues = match lockfile {
        Some(lockfile) => check_peer_dependencies_from_lockfile(
            lockfile,
            lockfile_dir,
            project_dirs,
            catalogs,
            config.resolve_peers_from_workspace_root,
        )?,
        None => IssuesByProjects::new(),
    };
    Ok(filter_peer_issues(issues, &config.peer_dependency_rules))
}
