use super::{
    InvocationIdentity, Path, ScriptIdentity, TaskGraph, TaskId, TaskIdentity, TaskNode,
    create_hex_hash,
};

pub(super) fn task_id(node: &TaskNode, workspace_dir: &Path) -> TaskId {
    let relative = pnpm_fs::relative_path(workspace_dir, &node.project);
    let project = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
    };
    TaskId {
        project,
        task: node.task_name.clone(),
    }
}

pub(super) fn task_identity(
    node: &TaskNode,
    id: TaskId,
    graph: &TaskGraph,
    workspace_dir: &Path,
    script_commands: &impl Fn(&TaskNode, &str) -> Vec<String>,
) -> TaskIdentity {
    let mut scripts: Vec<ScriptIdentity> = node.scripts
        .iter()
        .map(|name| ScriptIdentity {
            name: name.clone(),
            commands: script_commands(node, name),
        })
        .collect();
    scripts.sort_by(|left, right| left.name.cmp(&right.name));
    let mut dependencies: Vec<TaskId> = node.dependencies
        .iter()
        .map(|dependency| task_id(&graph[dependency], workspace_dir))
        .collect();
    dependencies.sort();
    TaskIdentity {
        project: id.project,
        task: id.task,
        scripts,
        requested: node.requested,
        dependencies,
    }
}

/// The invocation's identity hash over its command line and the sorted
/// settings and tasks.
pub(super) fn invocation_hash(
    command: &str,
    params: &[String],
    settings: &[String],
    mut tasks: Vec<TaskIdentity>,
) -> String {
    tasks.sort_by(|left, right| {
        left.project
            .cmp(&right.project)
            .then_with(|| left.task.cmp(&right.task))
    });
    let mut settings = settings.to_vec();
    settings.sort();
    let identity = serde_json::to_string(&InvocationIdentity {
        command,
        params,
        settings: &settings,
        tasks,
    })
    .expect("task invocation identity serializes");
    create_hex_hash(&identity)
}
