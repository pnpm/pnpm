use super::{
    BuildPipelineTaskGraphOptions, BuildTaskGraphOptions, HashSet, IndexMap, Path, PathBuf,
    TaskGraph, TaskKey, TaskNode, TaskSettings, VecDeque, task_concurrency,
};

/// Build the graph of tasks the invocation runs: a task named `task_name`
/// in every selected project, plus every task those transitively pull in
/// through `dependsOn`. A task with no `tasks` entry behaves as
/// `dependsOn: ['^<its own name>']`: plain topological order over the
/// project graph.
pub fn build_task_graph<SelectScripts>(
    options: &BuildTaskGraphOptions<'_, SelectScripts>,
) -> TaskGraph
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    let options = SeededBuildOptions {
        project_dependencies: options.project_dependencies,
        select_scripts: &options.select_scripts,
        task_names: std::slice::from_ref(&options.task_name),
        requested_projects: None,
        tasks: options.tasks,
        is_selector_task: options.is_selector_task,
    };
    options.build()
}

/// [`build_task_graph`] for a `pnpm pipeline` invocation, which requests
/// several task names at once across the requested projects.
pub fn build_pipeline_task_graph<SelectScripts>(
    options: &BuildPipelineTaskGraphOptions<'_, SelectScripts>,
) -> TaskGraph
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    let seeded = SeededBuildOptions {
        project_dependencies: options.project_dependencies,
        select_scripts: &options.select_scripts,
        task_names: options.task_names,
        requested_projects: options.requested_projects,
        tasks: options.tasks,
        is_selector_task: |_| false,
    };
    seeded.build()
}

struct SeededBuildOptions<'a, SelectScripts>
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    project_dependencies: &'a IndexMap<PathBuf, Vec<PathBuf>>,
    select_scripts: &'a SelectScripts,
    task_names: &'a [&'a str],
    requested_projects: Option<&'a [PathBuf]>,
    tasks: Option<&'a IndexMap<String, TaskSettings>>,
    is_selector_task: fn(&str) -> bool,
}

impl<SelectScripts> SeededBuildOptions<'_, SelectScripts>
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    fn build(&self) -> TaskGraph {
        let mut graph: TaskGraph = IndexMap::new();
        let mut queue = self.seed_queue();
        while let Some((project, task_name, requested)) = queue.pop_front() {
            let key = TaskKey { project: project.clone(), task_name: task_name.clone() };
            if let Some(existing) = graph.get_mut(&key) {
                existing.requested |= requested;
                continue;
            }
            let settings = self.task_settings(&task_name);
            let scripts = (self.select_scripts)(&project, &task_name);
            let dependencies = self.dependency_keys(&project, &task_name, settings, &scripts);
            queue.extend(
                dependencies
                    .iter()
                    .map(|dependency| {
                        (dependency.project.clone(), dependency.task_name.clone(), false)
                    }),
            );
            graph.insert(
                key,
                TaskNode {
                    project,
                    task_name,
                    concurrency: settings.and_then(task_concurrency),
                    scripts,
                    requested,
                    dependencies,
                },
            );
        }
        graph
    }

    fn task_settings(&self, task_name: &str) -> Option<&TaskSettings> {
        self.tasks.and_then(|tasks| tasks.get(task_name))
    }

    /// Every requested task of every seed project, which is either the
    /// explicitly requested projects or the whole workspace.
    fn seed_queue(&self) -> VecDeque<(PathBuf, String, bool)> {
        let seed_projects: Vec<&PathBuf> = match self.requested_projects {
            Some(requested) => requested.iter().collect(),
            None => self.project_dependencies.keys().collect(),
        };
        seed_projects
            .into_iter()
            .flat_map(|project| {
                self.task_names
                    .iter()
                    .map(|task_name| (project.clone(), (*task_name).to_string(), true))
            })
            .collect()
    }

    /// The tasks `task_name` at `project` depends on, in declaration order
    /// and deduplicated.
    fn dependency_keys(
        &self,
        project: &Path,
        task_name: &str,
        settings: Option<&TaskSettings>,
        scripts: &[String],
    ) -> Vec<TaskKey> {
        let entries: Vec<String> = match settings {
            Some(settings) => settings.depends_on.clone().unwrap_or_default(),
            None if (self.is_selector_task)(task_name) => self.selector_entries(scripts),
            None => vec![format!("^{task_name}")],
        };
        let mut dependencies: Vec<TaskKey> = Vec::new();
        let mut seen: HashSet<TaskKey> = HashSet::new();
        for entry in &entries {
            for dependency in self.entry_keys(entry, project) {
                if seen.insert(dependency.clone()) {
                    dependencies.push(dependency);
                }
            }
        }
        dependencies
    }

    /// The `dependsOn` entries of a `RegExp` selector task: the union of the
    /// entries of every script it selected in the project. A selector task
    /// runs its matched scripts as one unit, so a same-project entry naming
    /// one of them cannot order anything and is dropped; `^` entries fan
    /// out over the project graph as usual.
    fn selector_entries(&self, scripts: &[String]) -> Vec<String> {
        let mut entries: Vec<String> = Vec::new();
        for script in scripts {
            for entry in self.task_depends_on(script) {
                if !entry.starts_with('^') && scripts.contains(&entry) {
                    continue;
                }
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
        }
        entries
    }

    fn task_depends_on(&self, task_name: &str) -> Vec<String> {
        match self.task_settings(task_name) {
            Some(settings) => settings.depends_on.clone().unwrap_or_default(),
            None => vec![format!("^{task_name}")],
        }
    }

    /// The tasks one `dependsOn` entry names: a `^`-prefixed entry fans out
    /// over the project's dependencies, anything else names a task in the
    /// same project.
    fn entry_keys(&self, entry: &str, project: &Path) -> Vec<TaskKey> {
        let Some(dependency_task_name) = entry.strip_prefix('^') else {
            return vec![TaskKey { project: project.to_path_buf(), task_name: entry.to_string() }];
        };
        self.project_dependencies
            .get(project)
            .into_iter()
            .flatten()
            .map(|dependency_project| TaskKey {
                project: dependency_project.clone(),
                task_name: dependency_task_name.to_string(),
            })
            .collect()
    }
}
