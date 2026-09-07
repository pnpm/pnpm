use clap::{Args, CommandFactory};
use miette::{Context, IntoDiagnostic};

use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    IssuesByProjects, check_peer_dependencies_from_lockfile, filter_peer_issues, render_peer_issues,
};
use pnpm_lockfile::Lockfile;

use crate::cli_args::{
    catalogs::configured_catalogs,
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};

#[derive(Debug, Args)]
pub struct PeersArgs {
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
        match self.params.first().map(String::as_str) {
            Some("check") | None => {}
            Some(_) => {
                let mut cmd = crate::cli_args::CliArgs::command();
                cmd.build();
                let _ = cmd.find_subcommand_mut("peers").expect("peers subcommand").print_help();
                return Ok(PeersOutcome::UnknownSubcommand);
            }
        }

        let lockfile_dir = config.lockfile_dir_for(dir);
        let project_dirs = if recursive {
            let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
            let (projects, _) = discover_workspace_projects(workspace_root, config)?;
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?
                .selected
                .keys()
                .cloned()
                .collect()
        } else {
            vec![dir.to_path_buf()]
        };

        let lockfile = if self.lockfile_only {
            Lockfile::load_wanted_from_dir(lockfile_dir)
        } else {
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
                Ok(Some(lf)) => Ok(Some(lf)),
                Ok(None) => Lockfile::load_wanted_from_dir(lockfile_dir),
                Err(e) => Err(e),
            }
        }
        .into_diagnostic()
        .wrap_err("load lockfile")?;
        let catalogs = configured_catalogs(config)?;
        let catalogs =
            (config.workspace_dir.is_some() || config.catalogs.is_some()).then_some(&catalogs);

        // A missing lockfile yields empty issues, mirroring pnpm's
        // `checkPeerDependencies`, so both output modes stay on the common
        // path (`{}` for `--json`, "No peer dependency issues found" otherwise).
        let issues = match &lockfile {
            Some(lockfile) => check_peer_dependencies_from_lockfile(
                lockfile,
                lockfile_dir,
                &project_dirs,
                catalogs,
            )?,
            None => IssuesByProjects::new(),
        };
        let issues = filter_peer_issues(issues, &config.peer_dependency_rules);

        let no_issues = issues.values().all(|pi| pi.bad.is_empty() && pi.missing.is_empty());

        if self.json {
            let output = serde_json::to_string_pretty(&issues)
                .into_diagnostic()
                .wrap_err("serialize issues to JSON")?;
            println!("{output}");
        } else if no_issues {
            println!("No peer dependency issues found");
        } else {
            println!("Issues with peer dependencies found\n");
            println!("{}", render_peer_issues(&issues));
        }

        Ok(if no_issues { PeersOutcome::NoIssues } else { PeersOutcome::IssuesFound })
    }
}
