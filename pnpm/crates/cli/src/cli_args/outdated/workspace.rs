use super::{
    Arc, AutoExcludeRoot, Config, DependencyGroup, HashMap, IntoDiagnostic, Lockfile, OutdatedArgs,
    OutdatedFilters, OutdatedOutcome, OutdatedPackage, OutdatedQuery, OutdatedRun, PackageManifest,
    PathBuf, Reporter, State, collect_outdated, collect_outdated_for_importer_in_run,
    discover_workspace_projects, github_actions, select_recursive_projects, sort_outdated,
    sort_workspace_outdated,
};
use crate::cli_args::ecosystem_deps::{
    collect_ecosystem_outdated, declared_ecosystem_dep_names, discover_standalone_ecosystem_dirs,
    ecosystem_lockfile_state, ecosystem_project_name,
};
use std::collections::HashSet;

pub(super) struct OutdatedInWorkspace {
    pub(super) package: OutdatedPackage,
    pub(super) dependents: Vec<DependentProject>,
}

#[derive(Clone)]
pub(super) struct DependentProject {
    pub(super) name: String,
    pub(super) location: PathBuf,
}

/// The directory the manifest sits in, or its path when it has no parent.
pub(super) fn project_dir(manifest: &PackageManifest) -> &std::path::Path {
    manifest
        .path()
        .parent()
        .unwrap_or_else(|| manifest.path())
}

pub(super) fn loaded_lockfile(state: &State) -> miette::Result<Option<&Lockfile>> {
    state.lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))
}

pub(super) fn isolated_global_config(config: &Config) -> &'static Config {
    let mut isolated_config = config.clone();
    isolated_config.workspace_dir = None;
    isolated_config.shared_workspace_lockfile = false;
    isolated_config.lockfile_dir = None;
    isolated_config.lockfile = true;
    Config::leak(isolated_config)
}

pub(super) fn global_states(
    global_pkg_dir: &std::path::Path,
    config: &'static Config,
) -> miette::Result<Vec<State>> {
    pnpm_global::scan_global_packages(global_pkg_dir)
        .map_err(|err| miette::miette!("failed to scan global packages: {err}"))?
        .into_iter()
        .map(|pkg| {
            State::init(pkg.install_dir.join("package.json"), config, false)
                .map_err(|err| miette::Report::new(err).wrap_err("initialize global state"))
        })
        .collect()
}

/// Every selected project's outdated dependencies, grouped by package.
pub(super) async fn workspace_outdated(
    inputs: &ProjectOutdatedInputs<'_>,
    project_inputs: &[(&PathBuf, &pnpm_workspace::Project, Option<Lockfile>)],
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let project_queries = project_inputs
        .iter()
        .map(|(project_dir, project, project_lockfile)| {
            outdated_for_project(inputs, project_dir, project, project_lockfile.as_ref())
        });
    group_workspace_outdated(futures_util::future::join_all(project_queries).await)
}

pub(super) fn no_lockfile_error(dir: &std::path::Path) -> miette::Report {
    let dir = dir.display();
    miette::miette!(
        code = "ERR_PNPM_OUTDATED_NO_LOCKFILE",
        r#"No lockfile in directory "{dir}". Run `pnpm install` to generate one."#,
    )
}

/// The inputs every project's outdated query shares.
pub(super) async fn recursive_workspace_outdated(
    state: &State,
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    query: &OutdatedQuery<'_>,
    raw_packages: Vec<String>,
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let config = state.config;
    let shared_lockfile = if config.shares_one_lockfile() { loaded_lockfile(state)? } else { None };
    let project_inputs = recursive_project_inputs(config, selection)?;
    let run = OutdatedRun::new(config, Arc::clone(&state.http_client), query)?
        .with_raw_packages(raw_packages);
    let mut results = workspace_outdated(
        &ProjectOutdatedInputs {
            config,
            lockfile_root: state.lockfile_dir(),
            shared_lockfile,
            query,
            run: &run,
        },
        &project_inputs,
    )
    .await?;
    if config.filter.is_empty() && config.test_pattern.is_empty() {
        let existing: HashSet<PathBuf> = selection.selected
            .keys()
            .cloned()
            .collect();
        let workspace_root =
            config.workspace_dir.as_deref().unwrap_or_else(|| state.lockfile_dir());
        let standalone_dirs =
            discover_standalone_ecosystem_dirs(workspace_root, &existing, config).await?;
        let standalone_results =
            collect_standalone_ecosystem_outdated(&standalone_dirs, query, &run).await?;
        results.extend(standalone_results);
    }
    Ok(results)
}

async fn collect_standalone_ecosystem_outdated(
    dirs: &[PathBuf],
    query: &OutdatedQuery<'_>,
    run: &OutdatedRun,
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let mut project_results = Vec::new();
    for dir in dirs {
        let name = ecosystem_project_name(dir, &run.config)
            .unwrap_or_else(|| dir.to_string_lossy().into_owned());
        let outdated =
            collect_ecosystem_outdated(dir, Some(name.clone()), query, &run.raw_packages, run)
                .await?;
        if !outdated.is_empty() {
            project_results.push(Ok((outdated, DependentProject { name, location: dir.clone() })));
        }
    }
    group_workspace_outdated(project_results)
}

pub(super) struct ProjectOutdatedInputs<'a> {
    pub(super) config: &'a Config,
    pub(super) lockfile_root: &'a std::path::Path,
    pub(super) shared_lockfile: Option<&'a Lockfile>,
    pub(super) query: &'a OutdatedQuery<'a>,
    pub(super) run: &'a OutdatedRun,
}

async fn outdated_for_project(
    inputs: &ProjectOutdatedInputs<'_>,
    project_dir: &std::path::Path,
    project: &pnpm_workspace::Project,
    project_lockfile: Option<&Lockfile>,
) -> miette::Result<(Vec<OutdatedPackage>, DependentProject)> {
    let shares_one_lockfile = inputs.config.shares_one_lockfile();
    let (lockfile, importer_id) = if shares_one_lockfile {
        (
            inputs.shared_lockfile,
            pnpm_workspace::importer_id_from_root_dir(inputs.lockfile_root, project_dir),
        )
    } else {
        (project_lockfile, Lockfile::ROOT_IMPORTER_KEY.to_string())
    };
    ensure_project_lockfiles(inputs, project_dir, &project.manifest, lockfile)?;
    let project_outdated = collect_outdated_for_importer_in_run(
        &project.manifest,
        lockfile,
        &importer_id,
        inputs.query,
        inputs.run,
    )
    .await?;
    let dependent = DependentProject {
        name: project.manifest
            .value()
            .get("name")
            .and_then(|name| name.as_str())
            .map(str::to_owned)
            .or_else(|| ecosystem_project_name(project_dir, inputs.config))
            .unwrap_or_else(|| project_dir.to_string_lossy().into_owned()),
        location: project_dir.to_path_buf(),
    };
    Ok((project_outdated, dependent))
}

fn ensure_project_lockfiles(
    inputs: &ProjectOutdatedInputs<'_>,
    project_dir: &std::path::Path,
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
) -> miette::Result<()> {
    let has_non_tool_dep = manifest
        .dependencies(inputs.query.include_direct.iter().copied())
        .any(|(name, spec)| {
            !pnpm_tool_protocol::is_tool_specifier(spec) && inputs.query.matches_name(name)
        });
    if lockfile.is_none() && has_non_tool_dep {
        let dir =
            if inputs.config.shares_one_lockfile() { inputs.lockfile_root } else { project_dir };
        return Err(no_lockfile_error(dir));
    }
    let (has_eco_deps, has_eco_lock) = ecosystem_lockfile_state(project_dir, inputs.config)?;
    if has_eco_deps && !has_eco_lock {
        return Err(no_lockfile_error(project_dir));
    }
    Ok(())
}

pub(super) fn recursive_project_inputs<'a>(
    config: &Config,
    selection: &'a crate::cli_args::recursive::RecursiveSelection<'a>,
) -> miette::Result<Vec<(&'a PathBuf, &'a pnpm_workspace::Project, Option<Lockfile>)>> {
    let mut project_inputs = Vec::new();
    let all_groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    for (project_dir, node) in &selection.selected {
        let project = node.package.project;
        let has_any_dependency = project.manifest
            .dependencies(all_groups)
            .next()
            .is_some()
            || !crate::cli_args::engine_deps::declared_engine_deps(
                &project.manifest,
                config,
                &all_groups,
            )
            .is_empty()
            || !declared_ecosystem_dep_names(project_dir, config)?.is_empty();
        if !has_any_dependency {
            continue;
        }
        let project_lockfile = if config.shares_one_lockfile() {
            None
        } else {
            Lockfile::load_wanted_from_dir(project_dir).into_diagnostic()?
        };
        project_inputs.push((project_dir, project, project_lockfile));
    }
    Ok(project_inputs)
}

fn group_workspace_outdated(
    project_results: Vec<miette::Result<(Vec<OutdatedPackage>, DependentProject)>>,
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let mut outdated: Vec<OutdatedInWorkspace> = Vec::new();
    let mut outdated_indexes: HashMap<String, usize> = HashMap::new();
    for result in project_results {
        let (project_outdated, dependent) = result?;
        for package in project_outdated {
            let dependency_type: &'static str = package.belongs_to.into();
            let key =
                format!("{}\0{}\0{}", package.package_name, package.current_str(), dependency_type);
            if let Some(&index) = outdated_indexes.get(&key) {
                outdated[index].dependents.push(dependent.clone());
            } else {
                outdated_indexes.insert(key, outdated.len());
                outdated.push(OutdatedInWorkspace { package, dependents: vec![dependent.clone()] });
            }
        }
    }
    Ok(outdated)
}

impl OutdatedArgs {
    pub(super) async fn run_recursive<Reporter: self::Reporter>(
        self,
        state: State,
    ) -> miette::Result<OutdatedOutcome> {
        let config = state.config;
        let workspace_root = config.workspace_dir
            .clone()
            .unwrap_or_else(|| state.lockfile_dir().to_path_buf());
        let (projects, _) = discover_workspace_projects(&workspace_root, config)?;
        let selection = select_recursive_projects(
            &projects,
            config,
            project_dir(&state.manifest),
            AutoExcludeRoot::Disabled,
        )?;
        let package_patterns = self.package_patterns();
        let filters = OutdatedFilters::validated(
            &self,
            config,
            &package_patterns,
            selection.selected.values().map(|node| &node.package.project.manifest),
            true,
        )?;
        let mut outdated = recursive_workspace_outdated(
            &state,
            &selection,
            &filters.query(self.target_version()),
            package_patterns,
        )
        .await?;
        outdated.extend(
            self.workspace_outdated_actions::<Reporter>(config, &workspace_root, &filters.include)
                .await?,
        );

        sort_workspace_outdated(&mut outdated);
        self.write_recursive_rendered(&outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    async fn workspace_outdated_actions<Reporter: self::Reporter>(
        &self,
        config: &Config,
        workspace_root: &std::path::Path,
        include: &[DependencyGroup],
    ) -> miette::Result<Vec<OutdatedInWorkspace>> {
        let action_matcher = github_actions::selector_matcher(&self.packages);
        Ok(self
            .outdated_actions::<Reporter>(config, workspace_root, include, action_matcher.as_ref())
            .await?
            .into_iter()
            .map(|action| OutdatedInWorkspace {
                package: OutdatedPackage::from(action),
                dependents: vec![DependentProject {
                    name: ".github".to_string(),
                    location: workspace_root.to_path_buf(),
                }],
            })
            .collect())
    }

    pub async fn run_global(self, config: &'static Config) -> miette::Result<OutdatedOutcome> {
        let global_pkg_dir = config.global_pkg_dir
            .clone()
            .ok_or_else(|| {
                miette::miette!(
                    code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                    "Unable to find the global packages directory"
                )
            })?;
        let config = isolated_global_config(config);
        let package_patterns = self.package_patterns();
        let states = global_states(&global_pkg_dir, config)?;
        let filters = OutdatedFilters::validated(
            &self,
            config,
            &package_patterns,
            states.iter().map(|state| &state.manifest),
            false,
        )?;
        let query = filters.query(self.target_version());
        let mut outdated = Vec::new();
        for state in states {
            outdated.extend(
                collect_outdated(
                    &state.manifest,
                    loaded_lockfile(&state)?,
                    config,
                    &state.http_client,
                    &query,
                )
                .await?,
            );
        }

        sort_outdated(&mut outdated, self.output.sort_by);
        self.write_rendered(&outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }
}
