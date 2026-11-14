use crate::cli_args::{
    catalogs::configured_catalogs, global::GlobalError, listed_projects::listed_projects,
    recursive::selectors_narrow_the_run,
};
use clap::{Args, CommandFactory};
use miette::{Context, IntoDiagnostic};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    IssuesByProjects, LinkedLockfileLoader, LockfilePeerCheck,
    check_peer_dependencies_from_lockfile, filter_peer_issues,
};
use pnpm_global::{GlobalPackageInfo, scan_global_packages};
use pnpm_lockfile::{Lockfile, WantedLockfileSelection};
use report::{UndeclaredWarnings, report_issues, undeclared_warnings};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

mod report;

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
        report_issues(&issues, &undeclared, self.json)
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
        let catalogs = resolved_catalogs(config)?;
        let issues = checked_issues(
            config,
            lockfile_dir,
            &project_dirs,
            lockfile.as_ref(),
            catalogs.as_ref(),
            None,
        )?;
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
        let catalogs = resolved_catalogs(config)?;
        let mut cache = DedicatedLockfiles::new(self, config, &projects);
        let mut issues = IssuesByProjects::new();
        let mut undeclared = UndeclaredWarnings::new();
        for (project_dir, name) in projects {
            let project_key = if recursive {
                pnpm_workspace::importer_id_from_root_dir(workspace_root, &project_dir)
            } else {
                ".".to_string()
            };
            let (mut project_issues, mut project_undeclared) = dedicated_project_issues(
                &mut cache,
                &project_dir,
                name.as_deref(),
                catalogs.as_ref(),
            )?;
            if let Some(entry) = project_issues.remove(".") {
                issues.insert(project_key.clone(), entry);
            }
            if let Some(peers) = project_undeclared.remove(".") {
                undeclared.insert(project_key, peers);
            }
        }
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
                None,
                None,
            )?;
            if let Some(root_issues) = group_issues.remove(".") {
                issues.insert(global_group_key(&group, &issues), root_issues);
            }
        }
        Ok(issues)
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

struct DedicatedLockfiles<'a> {
    args: &'a PeersArgs,
    base_config: &'a Config,
    names_by_dir: HashMap<PathBuf, String>,
    by_dir: HashMap<PathBuf, Option<Arc<Lockfile>>>,
}

impl<'a> DedicatedLockfiles<'a> {
    fn new(
        args: &'a PeersArgs,
        base_config: &'a Config,
        projects: &[(PathBuf, Option<String>)],
    ) -> Self {
        let names_by_dir = projects
            .iter()
            .filter_map(|(dir, name)| Some((canonical_key(dir), name.clone()?)))
            .collect();
        Self { args, base_config, names_by_dir, by_dir: HashMap::new() }
    }

    fn load(
        &mut self,
        dir: &Path,
        name_hint: Option<&str>,
    ) -> miette::Result<Option<Arc<Lockfile>>> {
        let key = canonical_key(dir);
        if let Some(cached) = self.by_dir.get(&key) {
            return Ok(cached.clone());
        }
        let name = name_hint.or_else(|| self.names_by_dir.get(&key).map(String::as_str));
        let mut project_config = self.base_config.clone();
        project_config.anchor_dedicated_project(dir, name);
        let lockfile = self.args
            .load_lockfile(
                &project_config.install_state_dir,
                dir,
                &project_config.wanted_lockfile_selection(),
            )?
            .map(Arc::new);
        self.by_dir.insert(key, lockfile.clone());
        Ok(lockfile)
    }
}

fn canonical_key(dir: &Path) -> PathBuf {
    dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

fn dedicated_project_issues(
    cache: &mut DedicatedLockfiles<'_>,
    project_dir: &Path,
    name: Option<&str>,
    catalogs: Option<&Catalogs>,
) -> miette::Result<(IssuesByProjects, UndeclaredWarnings)> {
    let lockfile = cache.load(project_dir, name)?;
    let mut project_config = cache.base_config.clone();
    project_config.anchor_dedicated_project(project_dir, name);
    project_config.resolve_peers_from_workspace_root = false;
    let project_dirs = [project_dir.to_path_buf()];
    let mut sibling_err: Option<miette::Report> = None;
    let mut load_sibling = |sibling_dir: &Path, sibling_name: Option<&str>| match cache.load(
        sibling_dir,
        sibling_name,
    ) {
        Ok(loaded) => loaded,
        Err(err) => {
            sibling_err.get_or_insert(err);
            None
        }
    };
    let issues = checked_issues(
        &project_config,
        project_dir,
        &project_dirs,
        lockfile.as_deref(),
        catalogs,
        Some(&mut load_sibling),
    )?;
    if let Some(err) = sibling_err {
        return Err(err);
    }
    let undeclared =
        undeclared_warnings(&project_config, project_dir, &project_dirs, lockfile.as_deref());
    Ok((issues, undeclared))
}

fn resolved_catalogs(config: &Config) -> miette::Result<Option<Catalogs>> {
    if config.workspace_dir.is_none() && config.catalogs.is_none() {
        return Ok(None);
    }
    Ok(Some(configured_catalogs(config)?))
}

fn checked_issues(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
    lockfile: Option<&Lockfile>,
    catalogs: Option<&Catalogs>,
    linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>>,
) -> miette::Result<IssuesByProjects> {
    let issues = match lockfile {
        Some(lockfile) => check_peer_dependencies_from_lockfile(
            LockfilePeerCheck {
                lockfile,
                lockfile_dir,
                workspace_dir: config.workspace_dir.as_deref(),
                project_dirs,
                catalogs,
                resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
            },
            linked_lockfiles,
        )?,
        None => IssuesByProjects::new(),
    };
    Ok(filter_peer_issues(issues, &config.peer_dependency_rules))
}
