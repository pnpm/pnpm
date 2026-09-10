use super::{
    Config, DependencyGroup, HashMap, IntoDiagnostic, Lockfile, OutdatedPackage, OutdatedQuery,
    OutdatedRun, PackageManifest, PathBuf, State, collect_outdated_for_importer_in_run,
};

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
    manifest.path().parent().unwrap_or_else(|| manifest.path())
}

pub(super) fn loaded_lockfile(state: &State) -> miette::Result<Option<&Lockfile>> {
    state.lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))
}

/// The pnpm home may contain a workspace config, but each global install
/// group owns its package.json and lockfile. Keep project lookup anchored
/// to the scanned group while retaining the global registry and network
/// settings. A group's lockfile is written unconditionally
/// (`run_group_install` forces it) because it is where the installed
/// versions are recorded, so reading it back must not depend on the
/// caller's `lockfile` setting.
pub(super) fn isolated_global_config(config: &Config) -> &'static Config {
    let mut isolated_config = config.clone();
    isolated_config.workspace_dir = None;
    isolated_config.shared_workspace_lockfile = false;
    isolated_config.lockfile_dir = None;
    isolated_config.lockfile = true;
    Config::leak(isolated_config)
}

/// Every selected project's outdated dependencies, grouped by package.
pub(super) async fn workspace_outdated(
    inputs: &ProjectOutdatedInputs<'_>,
    project_inputs: &[(&PathBuf, &pnpm_workspace::Project, Option<Lockfile>)],
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let project_queries = project_inputs.iter().map(|(project_dir, project, project_lockfile)| {
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
pub(super) struct ProjectOutdatedInputs<'a> {
    pub(super) config: &'a Config,
    /// The directory the importer ids name projects relative to, which
    /// `lockfileDir` can pin somewhere other than the workspace root.
    pub(super) lockfile_root: &'a std::path::Path,
    pub(super) shared_lockfile: Option<&'a Lockfile>,
    pub(super) query: &'a OutdatedQuery<'a>,
    pub(super) run: &'a OutdatedRun,
}

/// One project's outdated packages, and the dependent entry the report
/// lists it under.
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
    let Some(lockfile) = lockfile else {
        let lockfile_dir = if shares_one_lockfile { inputs.lockfile_root } else { project_dir };
        return Err(no_lockfile_error(lockfile_dir));
    };
    let project_outdated = collect_outdated_for_importer_in_run(
        &project.manifest,
        Some(lockfile),
        &importer_id,
        inputs.query,
        inputs.run,
    )
    .await?;
    let dependent = DependentProject {
        name: project
            .manifest
            .value()
            .get("name")
            .and_then(|name| name.as_str())
            .map_or_else(|| project_dir.to_string_lossy().into_owned(), str::to_owned),
        location: project_dir.to_path_buf(),
    };
    Ok((project_outdated, dependent))
}

/// The selected projects that declare at least one dependency, with the
/// lockfile each one reads. A project with no dependencies has nothing
/// to report and needs no lockfile.
pub(super) fn recursive_project_inputs<'a>(
    config: &Config,
    selection: &'a crate::cli_args::recursive::RecursiveSelection<'a>,
) -> miette::Result<Vec<(&'a PathBuf, &'a pnpm_workspace::Project, Option<Lockfile>)>> {
    let mut project_inputs = Vec::new();
    for (project_dir, node) in &selection.selected {
        let project = node.package.project;
        let has_any_dependency = project
            .manifest
            .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
            .next()
            .is_some();
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

/// Collect the per-project results into one report, listing every
/// project that depends on each outdated package.
fn group_workspace_outdated(
    project_results: Vec<miette::Result<(Vec<OutdatedPackage>, DependentProject)>>,
) -> miette::Result<Vec<OutdatedInWorkspace>> {
    let mut outdated: Vec<OutdatedInWorkspace> = Vec::new();
    let mut outdated_indexes: HashMap<String, usize> = HashMap::new();
    for result in project_results {
        let (project_outdated, dependent) = result?;
        for package in project_outdated {
            let dependency_type: &'static str = package.belongs_to.into();
            let key = format!("{}\0{}\0{}", package.package_name, package.current, dependency_type);
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
