use super::{TaskRunExecutionSettings, TaskRunStateContext, task_run_execution_settings};
use indexmap::IndexMap;
use pnpm_workspace_task_scheduler::{TaskGraph, TaskKey, TaskNode};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

#[test]
fn task_execution_settings_have_a_stable_canonical_encoding() {
    let extra_bin_paths = vec![PathBuf::from("tools"), PathBuf::from("other-tools")];
    let extra_env = HashMap::from([
        ("ZED".to_string(), "last".to_string()),
        ("ALPHA".to_string(), "first".to_string()),
    ]);
    assert_eq!(
        task_run_execution_settings(&TaskRunExecutionSettings {
            extra_bin_paths: &extra_bin_paths,
            extra_env: &extra_env,
            modules_dir: Path::new("vendor"),
            node_experimental_package_map: true,
            node_options: Some("--conditions=development"),
            user_agent: "pnpm/test",
        }),
        [
            r#"extra-bin-paths=["tools","other-tools"]"#,
            r#"extra-env=[["ALPHA","first"],["ZED","last"]]"#,
            "modules-dir=vendor",
            "node-experimental-package-map=true",
            "node-options=--conditions=development",
            "user-agent=pnpm/test",
        ],
    );
}

#[test]
fn a_changed_execution_setting_produces_a_different_invocation_identity() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key,
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let settings = |mode: &str| {
        let extra_env = HashMap::from([("MODE".to_string(), mode.to_string())]);
        task_run_execution_settings(&TaskRunExecutionSettings {
            extra_bin_paths: &[],
            extra_env: &extra_env,
            modules_dir: Path::new("node_modules"),
            node_experimental_package_map: false,
            node_options: None,
            user_agent: "",
        })
    };
    let first = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &settings("first"),
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let second = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &settings("second"),
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );

    assert_ne!(first.invocation, second.invocation);
}

#[test]
fn ignores_a_torn_trailing_record_and_removes_a_completed_journal() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let first = workspace.path().join("first");
    let second = workspace.path().join("second");
    let first_key = TaskKey { project: first.clone(), task_name: "build".to_string() };
    let second_key = TaskKey { project: second.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([
        (
            first_key.clone(),
            TaskNode {
                project: first,
                task_name: "build".to_string(),
                concurrency: None,
                scripts: vec!["build".to_string()],
                requested: true,
                dependencies: Vec::new(),
            },
        ),
        (
            second_key.clone(),
            TaskNode {
                project: second,
                task_name: "build".to_string(),
                concurrency: None,
                scripts: vec!["build".to_string()],
                requested: true,
                dependencies: vec![first_key.clone()],
            },
        ),
    ]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    assert_eq!(
        context.invocation,
        "76a575bfcc3b67becd98b5ec661be54567b53954a723da167603dad119fab140",
    );
    let state = context.start(&HashSet::from([first_key.clone()])).expect("start state");
    state
        .record_passed(&second_key, &graph[&second_key], workspace.path())
        .expect("record passed task");
    fs::OpenOptions::new()
        .append(true)
        .open(&state.file_path)
        .expect("open state")
        .write_all(b"{\"run\":\"superseded\",\"project\":\"unknown\",\"task\":\"build\"}\n")
        .expect("write superseded record");
    fs::OpenOptions::new()
        .append(true)
        .open(&state.file_path)
        .expect("open state")
        .write_all(br#"{"project":"torn"#)
        .expect("write torn record");

    let completed =
        context.read_completed_tasks().expect("read state").expect("state is compatible");
    assert_eq!(completed, HashSet::from([first_key, second_key]));
    state.finish().expect("finish state");
    assert!(context.read_completed_tasks().expect("read finished state").is_none());
}

#[test]
fn rejects_a_malformed_complete_record() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key,
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let state = context.start(&HashSet::new()).expect("start state");
    fs::OpenOptions::new()
        .append(true)
        .open(&state.file_path)
        .expect("open state")
        .write_all(b"not-json\n")
        .expect("write malformed record");

    assert!(context.read_completed_tasks().expect("read malformed state").is_none());
}

#[cfg(unix)]
#[test]
fn rejects_a_symlinked_state_directory() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().expect("create workspace");
    let outside = tempfile::tempdir().expect("create outside directory");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key,
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let node_modules = workspace.path().join("node_modules");
    fs::create_dir(&node_modules).expect("create node_modules");
    symlink(outside.path(), node_modules.join(".pnpm-task-run-state-v1"))
        .expect("symlink state directory");
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );

    let Err(error) = context.start(&HashSet::new()) else {
        panic!("symlinked state directory must be rejected");
    };
    assert!(
        error.to_string().contains("symbolic link or not a directory"),
        "unexpected error: {error:?}",
    );
    assert!(!outside.path().join("latest.json").exists());
}

#[cfg(unix)]
#[test]
fn disables_state_when_node_modules_is_read_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key.clone(),
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let node_modules = workspace.path().join("node_modules");
    fs::create_dir(&node_modules).expect("create node_modules");
    fs::set_permissions(&node_modules, fs::Permissions::from_mode(0o555))
        .expect("make node_modules read-only");
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );

    let result = context.start(&HashSet::new()).and_then(|state| {
        state.record_passed(&key, &graph[&key], workspace.path())?;
        state.finish()
    });
    fs::set_permissions(&node_modules, fs::Permissions::from_mode(0o755))
        .expect("restore node_modules permissions");

    result.expect("read-only state storage is optional");
    assert!(!context.latest_state_path.exists());
}

#[test]
fn finishing_an_older_invocation_preserves_the_newer_invocation_journal() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key.clone(),
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let older = context.start(&HashSet::new()).expect("start older state");
    let newer = context.start(&HashSet::from([key.clone()])).expect("start newer state");

    older.finish().expect("finish older state");

    let completed = context
        .read_completed_tasks()
        .expect("read newer state")
        .expect("newer state remains compatible");
    assert_eq!(completed, HashSet::from([key]));
    newer.finish().expect("finish newer state");
    assert!(context.read_completed_tasks().expect("read finished state").is_none());
}

#[cfg(unix)]
#[test]
fn a_finished_journal_is_not_resumable_when_cleanup_is_unavailable() {
    use std::os::unix::fs::PermissionsExt as _;

    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key.clone(),
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let state = context.start(&HashSet::from([key])).expect("start state");
    fs::set_permissions(&context.state_dir, fs::Permissions::from_mode(0o555))
        .expect("make state directory read-only");

    let result = state.finish();
    fs::set_permissions(&context.state_dir, fs::Permissions::from_mode(0o755))
        .expect("restore state directory permissions");

    result.expect("unavailable cleanup is optional");
    assert!(context.read_completed_tasks().expect("read finished state").is_none());
}

#[test]
fn a_stale_pointer_does_not_hide_a_newer_published_invocation_journal() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key.clone(),
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let older = context.start(&HashSet::new()).expect("start older state");
    let older_contents = fs::read_to_string(&older.file_path).expect("read older state");
    let older_header = older_contents.lines().next().expect("older state has a header");
    let newer = context.start(&HashSet::from([key.clone()])).expect("start newer state");

    fs::remove_file(&newer.published_path).expect("remove publication marker");
    fs::write(&context.latest_state_path, older_header).expect("write stale pointer");
    let completed = context
        .read_completed_tasks()
        .expect("read older state")
        .expect("older state remains compatible");
    assert!(completed.is_empty());
    fs::write(&newer.published_path, []).expect("write publication marker");

    let completed = context
        .read_completed_tasks()
        .expect("read newer state")
        .expect("newer state remains compatible");
    assert_eq!(completed, HashSet::from([key.clone()]));
    older.finish().expect("finish older state");
    let completed = context
        .read_completed_tasks()
        .expect("read newer state")
        .expect("newer state remains compatible");
    assert_eq!(completed, HashSet::from([key]));
    newer.finish().expect("finish newer state");
}

#[test]
fn a_stale_start_cannot_revive_state_after_a_newer_invocation_finishes() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let project = workspace.path().join("project");
    let key = TaskKey { project: project.clone(), task_name: "build".to_string() };
    let graph: TaskGraph = IndexMap::from([(
        key.clone(),
        TaskNode {
            project,
            task_name: "build".to_string(),
            concurrency: None,
            scripts: vec!["build".to_string()],
            requested: true,
            dependencies: Vec::new(),
        },
    )]);
    let context = TaskRunStateContext::new(
        "run",
        &["build".to_string()],
        &[],
        &graph,
        workspace.path(),
        |_, _| vec!["build-command".to_string()],
    );
    let older = context.start(&HashSet::new()).expect("start older state");
    let older_contents = fs::read(&older.file_path).expect("read older state");
    let older_header = older_contents
        .split(|byte| *byte == b'\n')
        .next()
        .expect("older state has a header")
        .to_vec();
    let newer = context.start(&HashSet::from([key])).expect("start newer state");

    newer.finish().expect("finish newer state");
    fs::write(&older.file_path, older_contents).expect("republish older journal");
    fs::write(&older.published_path, []).expect("republish older marker");
    fs::write(&context.latest_state_path, older_header).expect("write stale pointer");

    assert!(context.read_completed_tasks().expect("read finished state").is_none());
}
