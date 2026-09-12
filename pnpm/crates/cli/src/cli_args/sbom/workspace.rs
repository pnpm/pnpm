use super::{
    AutoExcludeRoot, Config, Display, Entry, Hash, HashMap, HashSet, LazyLockfile, Lockfile,
    PackageKey, Path, PathBuf, SnapshotEntry, State, discover_workspace_projects,
    importer_root_dir, no_projects_matched_message, notice_workspace_dir,
    select_recursive_projects, selected_importer_ids, validate_importer_id,
};

/// Whether the run asked for a subset of the workspace: any `--filter` /
/// `--filter-prod` selector, or `--workspace-root`. Without one, every
/// importer in the lockfile is in scope.
pub(super) fn selectors_narrow_the_run(config: &Config) -> bool {
    !config.filter.is_empty() || !config.filter_prod.is_empty() || config.workspace_root
}

/// The lockfile importer ids of the workspace projects the run's selectors
/// selected.
fn selected_workspace_importer_ids(state: &State) -> miette::Result<HashSet<String>> {
    let project_dir = state.project_dir();
    let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(project_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
    let selection =
        select_recursive_projects(&projects, state.config, project_dir, AutoExcludeRoot::Disabled)?;
    Ok(selected_importer_ids(&selection, state.lockfile_dir()).into_iter().collect())
}

/// The selected importer ids the lockfile has no entry for, sorted so the
/// error names them in a stable order.
fn missing_importers(selected: &HashSet<String>, lockfile_ids: &[String]) -> Vec<String> {
    let known: HashSet<&str> = lockfile_ids.iter().map(String::as_str).collect();
    let mut missing: Vec<String> =
        selected.iter().filter(|id| !known.contains(id.as_str())).cloned().collect();
    missing.sort_unstable();
    missing
}

fn missing_importers_error(missing: &[String], project_kind: &str) -> miette::Report {
    let plural = if missing.len() == 1 { "" } else { "s" };
    let names = missing.join(", ");
    let lockfile_name = pnpm_lockfile::Lockfile::FILE_NAME;
    miette::miette!(
        code = "ERR_PNPM_SBOM_MISSING_IMPORTERS",
        r#"{lockfile_name} has no entry for the {project_kind} workspace project{plural}: {names}. Run "pnpm install" to update it."#,
    )
}

fn extend_dedicated_lockfile_map<Key, Value>(
    current: &mut HashMap<Key, Value>,
    incoming: HashMap<Key, Value>,
    entry_kind: &str,
    selected_dir: &Path,
) -> miette::Result<()>
where
    Key: Display + Eq + Hash,
    Value: PartialEq,
{
    for (key, value) in incoming {
        match current.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(entry) if entry.get() != &value => {
                let key = entry.key();
                let selected_dir = selected_dir.display();
                return Err(miette::miette!(
                    code = "ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES",
                    "Cannot combine dedicated workspace lockfiles because {} contains a different {entry_kind} entry for {key}",
                    selected_dir,
                ));
            }
            Entry::Occupied(_) => {}
        }
    }
    Ok(())
}

fn extend_dedicated_snapshots(
    current: &mut HashMap<PackageKey, SnapshotEntry>,
    incoming: HashMap<PackageKey, SnapshotEntry>,
    selected_dir: &Path,
) -> miette::Result<()> {
    for (key, mut value) in incoming {
        match current.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                let incoming_optional = value.optional;
                value.optional = entry.get().optional;
                if entry.get() != &value {
                    let key = entry.key();
                    let selected_dir = selected_dir.display();
                    return Err(miette::miette!(
                        code = "ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES",
                        "Cannot combine dedicated workspace lockfiles because {} contains a different snapshot entry for {key}",
                        selected_dir,
                    ));
                }
                entry.get_mut().optional &= incoming_optional;
            }
        }
    }
    Ok(())
}

fn extend_dedicated_lockfile(
    current: &mut Lockfile,
    incoming: Lockfile,
    selected_dir: &Path,
) -> miette::Result<()> {
    extend_dedicated_lockfile_map(
        &mut current.importers,
        incoming.importers,
        "importer",
        selected_dir,
    )?;
    if let Some(packages) = incoming.packages {
        extend_dedicated_lockfile_map(
            current.packages.get_or_insert_default(),
            packages,
            "package",
            selected_dir,
        )?;
    }
    if let Some(snapshots) = incoming.snapshots {
        extend_dedicated_snapshots(
            current.snapshots.get_or_insert_default(),
            snapshots,
            selected_dir,
        )?;
    }
    Ok(())
}

fn selected_and_reachable_project_dirs(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
) -> Vec<PathBuf> {
    let graph = selection.full_graph();
    let mut project_dirs: Vec<PathBuf> = selection.selected.keys().cloned().collect();
    let mut seen: HashSet<PathBuf> = project_dirs.iter().cloned().collect();
    let mut index = 0;
    while let Some(project_dir) = project_dirs.get(index) {
        index += 1;
        let Some(project) = graph.get(project_dir) else {
            continue;
        };
        for dependency_dir in &project.dependencies {
            if seen.insert(dependency_dir.clone()) {
                project_dirs.push(dependency_dir.clone());
            }
        }
    }
    project_dirs
}

pub(super) fn merged_dedicated_lockfile_state(
    mut state: State,
) -> miette::Result<(State, Vec<PathBuf>)> {
    let project_dir = state.project_dir();
    let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(project_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
    let selection =
        select_recursive_projects(&projects, state.config, project_dir, AutoExcludeRoot::Disabled)?;

    let mut merged: Option<Lockfile> = None;
    let project_dirs = selected_and_reachable_project_dirs(&selection);
    let required_importer_ids: HashSet<String> = project_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect();
    let mut virtual_store_dirs = Vec::with_capacity(project_dirs.len());
    for selected_dir in &project_dirs {
        virtual_store_dirs.push(anchored_virtual_store_dir(state.config, selected_dir));

        let Some(mut lockfile) =
            Lockfile::load_wanted(selected_dir, &state.config.wanted_lockfile_selection())
                .map_err(miette::Report::new)?
        else {
            continue;
        };
        rekey_importers(&mut lockfile, selected_dir, workspace_root)?;
        if let Some(current) = &mut merged {
            extend_dedicated_lockfile(current, lockfile, selected_dir)?;
        } else {
            merged = Some(lockfile);
        }
    }

    assert_required_importers(merged.as_ref(), &required_importer_ids)?;

    virtual_store_dirs.sort_unstable();
    virtual_store_dirs.dedup();
    let mut config = state.config.clone();
    config.lockfile_dir = Some(workspace_root.to_path_buf());
    state.config = Config::leak(config);
    state.lockfile = LazyLockfile::preloaded(merged);
    Ok((state, virtual_store_dirs))
}

/// The virtual store dir the project's own lockfile anchors at.
fn anchored_virtual_store_dir(config: &Config, project_dir: &Path) -> PathBuf {
    let mut project_config = config.clone();
    project_config.anchor_lockfile_paths(project_dir);
    project_config.effective_virtual_store_dir().to_path_buf()
}

/// Re-key a dedicated lockfile's importers from its own root to the
/// workspace root.
fn rekey_importers(
    lockfile: &mut Lockfile,
    selected_dir: &Path,
    workspace_root: &Path,
) -> miette::Result<()> {
    let importers = std::mem::take(&mut lockfile.importers);
    lockfile.importers = importers
        .into_iter()
        .map(|(importer_id, importer)| {
            validate_importer_id(&importer_id).map_err(miette::Report::new)?;
            let importer_dir = importer_root_dir(selected_dir, &importer_id);
            let workspace_id =
                pnpm_workspace::importer_id_from_root_dir(workspace_root, &importer_dir);
            Ok((workspace_id, importer))
        })
        .collect::<miette::Result<_>>()?;
    Ok(())
}

fn assert_required_importers(
    merged: Option<&Lockfile>,
    required_importer_ids: &HashSet<String>,
) -> miette::Result<()> {
    let Some(lockfile) = merged else {
        return Ok(());
    };
    let importer_ids: Vec<String> = lockfile.importers.keys().cloned().collect();
    let missing = missing_importers(required_importer_ids, &importer_ids);
    if !missing.is_empty() {
        return Err(missing_importers_error(&missing, "selected or reachable"));
    }
    Ok(())
}

/// `importers` is a `HashMap`, so its iteration order is arbitrary.
/// Sorting fixes the order `--split` emits its SBOMs in, and matches
/// the lockfile, whose importers are serialized sorted by id.
pub(super) fn sorted_importer_ids(lockfile: Option<&Lockfile>) -> Vec<String> {
    let mut all_importer_ids: Vec<String> =
        lockfile.map(|lf| lf.importers.keys().cloned().collect()).unwrap_or_default();
    all_importer_ids.sort_unstable();
    all_importer_ids
}

/// The importers the SBOM covers, in lockfile order. `None` when the
/// run's selectors matched no project: pnpm skips such a command, so an
/// SBOM of no project is never written.
pub(super) fn select_importer_ids(
    state: &State,
    all_importer_ids: Vec<String>,
    has_lockfile: bool,
) -> miette::Result<Option<Vec<String>>> {
    if !selectors_narrow_the_run(state.config) {
        return Ok(Some(all_importer_ids));
    }
    let selected = selected_workspace_importer_ids(state)?;
    if selected.is_empty() {
        let workspace_dir = notice_workspace_dir(state.config, state.project_dir());
        println!("{}", no_projects_matched_message(workspace_dir));
        return Ok(None);
    }
    // Selecting through the workspace can name a project the lockfile has
    // no importer for, which only an out-of-date lockfile produces — pnpm
    // writes an entry for every project, `{}` for one with no
    // dependencies. Walking what is left would answer with an SBOM that
    // under-reports the selection's dependencies, so the run fails
    // instead. No lockfile at all is a different failure, left to
    // `collect_components` so it keeps its own error.
    let missing =
        if has_lockfile { missing_importers(&selected, &all_importer_ids) } else { Vec::new() };
    if !missing.is_empty() {
        return Err(missing_importers_error(&missing, "selected"));
    }
    // Intersecting rather than mapping the selection keeps the lockfile
    // order established by the caller.
    Ok(Some(all_importer_ids.into_iter().filter(|id| selected.contains(id)).collect()))
}

pub(super) fn required_sbom_lockfile(state: &State) -> miette::Result<&Lockfile> {
    let lockfile = state
        .lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

    let Some(lockfile) = lockfile else {
        return Err(miette::miette!(
            code = "ERR_PNPM_SBOM_NO_LOCKFILE",
            "No pnpm-lock.yaml found: cannot generate SBOM without a lockfile"
        ));
    };

    Ok(lockfile)
}
