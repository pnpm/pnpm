use super::{
    Command, Config, CreateProjectsGraphOptions, GetChangedProjectsOptions, GraphPkg, HashMap,
    HashSet, LogEvent, LogLevel, Path, PathBuf, PnpmLog, Project, ProjectGraph,
    create_projects_graph, get_changed_projects,
};

/// The identity runs are recorded under on the server: the workspace
/// directory's name plus the same path hash that keys the local pipeline
/// data, so two checkouts of one repository stay distinguishable.
pub(super) fn workspace_identity(workspace_root: &Path) -> String {
    let directory_name = workspace_root.file_name().map(|name| name.to_string_lossy().into_owned());
    let spelled = directory_name.unwrap_or_default();
    let basename: String = spelled
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(50)
        .collect();
    let slug = pnpm_crypto_hash::create_short_hash(&workspace_root.to_string_lossy());
    let basename = basename.trim_start_matches('.');
    if basename.is_empty() { slug } else { format!("{basename}-{slug}") }
}

pub(super) fn build_full_graph<'a>(
    projects: &'a [Project],
    config: &Config,
) -> ProjectGraph<GraphPkg<'a>> {
    let graph_options = CreateProjectsGraphOptions {
        link_workspace_packages: Some(
            config.link_workspace_packages != pnpm_config::LinkWorkspacePackages::Off,
        ),
        ..CreateProjectsGraphOptions::default()
    };
    create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &graph_options,
    )
    .graph
}

/// How the run decided what to cover.
pub struct Selection {
    /// The projects whose pipeline tasks the run requests: the changed
    /// projects and their dependents. Root tasks participate when
    /// `includeWorkspaceRoot` is enabled.
    pub requested: Vec<PathBuf>,
    /// The dependency closure of `requested` — the projects the task
    /// graph spans. The extra projects participate only through
    /// `dependsOn` edges (an upstream build pulled in without its lint or
    /// tests), which is what keeps a task's cache key independent of how
    /// the run was narrowed, and what guarantees a selected build's
    /// upstream outputs exist on a fresh machine.
    pub selected: HashSet<PathBuf>,
    pub mode: SelectionMode,
    pub merge_base: Option<String>,
    pub changed_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Changed projects and their dependents.
    Affected,
    /// Every project: `--full`, an unresolvable base, or a change to
    /// files no project selection can attribute (the workspace root).
    Full,
}

pub(super) struct SelectAffectedOptions<'a> {
    pub(super) graph: &'a ProjectGraph<GraphPkg<'a>>,
    pub(super) workspace_root: &'a Path,
    pub(super) base: &'a str,
    pub(super) full: bool,
    pub(super) config: &'a Config,
    pub(super) emit: fn(&LogEvent),
}

pub(super) fn select_affected_projects(
    options: &SelectAffectedOptions<'_>,
) -> miette::Result<Selection> {
    let all_dirs: Vec<PathBuf> = options
        .graph
        .keys()
        .filter(|dir| {
            options.config.include_workspace_root || dir.as_path() != options.workspace_root
        })
        .cloned()
        .collect();

    if options.full {
        return Ok(full_selection(&all_dirs, None, 0));
    }
    let Some(merge_base) = resolve_merge_base(options.workspace_root, options.base) else {
        (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                "Cannot resolve the merge base of HEAD and {}; running the pipeline over every project.",
                options.base,
            ),
            prefix: options.workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(&all_dirs, None, 0));
    };

    select_changed_projects(options, &all_dirs, merge_base)
}

/// The changed projects and everything that depends on them, directly or
/// not.
fn projects_with_dependents(
    graph: &ProjectGraph<GraphPkg<'_>>,
    changed_projects: &[PathBuf],
) -> HashSet<PathBuf> {
    let mut dependents: HashMap<&Path, Vec<&Path>> = HashMap::new();
    for (dir, node) in graph {
        for dependency in &node.dependencies {
            dependents.entry(dependency.as_path()).or_default().push(dir.as_path());
        }
    }
    let mut affected: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<&Path> = changed_projects.iter().map(PathBuf::as_path).collect();
    while let Some(dir) = stack.pop() {
        if !affected.insert(dir.to_path_buf()) {
            continue;
        }
        stack.extend(dependents.get(dir).into_iter().flatten());
    }
    affected
}

/// The task graph additionally spans the affected set's transitive
/// dependencies; see [`Selection::selected`] for why.
fn with_transitive_dependencies(
    graph: &ProjectGraph<GraphPkg<'_>>,
    affected: &HashSet<PathBuf>,
    workspace_root: &Path,
    config: &Config,
) -> HashSet<PathBuf> {
    let mut selected = affected.clone();
    let mut stack: Vec<PathBuf> = affected.iter().cloned().collect();
    while let Some(dir) = stack.pop() {
        let dependencies =
            graph.get(&dir).map(|node| node.dependencies.as_slice()).unwrap_or_default();
        for dependency in dependencies {
            if (config.include_workspace_root || dependency.as_path() != workspace_root)
                && selected.insert(dependency.clone())
            {
                stack.push(dependency.clone());
            }
        }
    }
    selected
}

/// `merge-base(HEAD, base)`, deepening a shallow clone until the merge
/// base is reachable. `None` when it cannot be resolved (an unknown ref,
/// unrelated histories, not a git repository) — the caller falls back to
/// the full graph.
fn resolve_merge_base(workspace_root: &Path, base: &str) -> Option<String> {
    for _ in 0..5 {
        if let Some(sha) = git_stdout(workspace_root, &["merge-base", "HEAD", base]) {
            return Some(sha);
        }
        let shallow = git_stdout(workspace_root, &["rev-parse", "--is-shallow-repository"]);
        if shallow.as_deref() != Some("true") {
            return None;
        }
        let _ = Command::new("git")
            .args(["fetch", "--deepen=200"])
            .current_dir(workspace_root)
            .status();
    }
    None
}

pub(super) fn git_stdout(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn select_changed_projects(
    options: &SelectAffectedOptions<'_>,
    all_dirs: &[PathBuf],
    merge_base: String,
) -> miette::Result<Selection> {
    let changed = get_changed_projects(
        options.graph.keys().cloned().collect(),
        &merge_base,
        &GetChangedProjectsOptions {
            workspace_dir: options.workspace_root,
            test_pattern: &options.config.test_pattern,
            changed_files_ignore_pattern: &options.config.changed_files_ignore_pattern,
        },
    )
    .map_err(miette::Report::new)?;
    let changed_count =
        changed.changed_projects.len() + changed.ignore_dependent_for_projects.len();

    // A changed file above every package maps to the workspace root
    // project: the root manifest, the lockfile, a shared config. Those
    // feed every project in ways project topology cannot see, so pruning
    // is disabled for the run rather than guessed at.
    if changed
        .changed_projects
        .iter()
        .chain(&changed.ignore_dependent_for_projects)
        .any(|dir| dir == options.workspace_root)
    {
        (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message:
                "The diff touches workspace-root files; running the pipeline over every project."
                    .to_string(),
            prefix: options.workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(all_dirs, Some(merge_base), changed_count));
    }

    Ok(affected_selection(options, &changed, merge_base, changed_count))
}

fn full_selection(
    all_dirs: &[PathBuf],
    merge_base: Option<String>,
    changed_count: usize,
) -> Selection {
    Selection {
        requested: all_dirs.to_vec(),
        selected: all_dirs.iter().cloned().collect(),
        mode: SelectionMode::Full,
        merge_base,
        changed_count,
    }
}

fn affected_selection(
    options: &SelectAffectedOptions<'_>,
    changed: &pnpm_workspace_projects_filter::ChangedProjects,
    merge_base: String,
    changed_count: usize,
) -> Selection {
    let mut affected = projects_with_dependents(options.graph, &changed.changed_projects);
    // A project whose only changes match `testPattern` is selected itself
    // without pulling in its dependents.
    affected.extend(changed.ignore_dependent_for_projects.iter().cloned());
    if !options.config.include_workspace_root {
        affected.remove(options.workspace_root);
    }
    let selected = with_transitive_dependencies(
        options.graph,
        &affected,
        options.workspace_root,
        options.config,
    );

    // In the workspace graph's deterministic order, which is the
    // dispatch tie-break order.
    let requested: Vec<PathBuf> =
        options.graph.keys().filter(|dir| affected.contains(dir.as_path())).cloned().collect();
    Selection {
        requested,
        selected,
        mode: SelectionMode::Affected,
        merge_base: Some(merge_base),
        changed_count,
    }
}
