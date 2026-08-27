use super::TaskRunStateContext;
use indexmap::IndexMap;
use pnpm_workspace_task_scheduler::{TaskGraph, TaskKey, TaskNode};
use std::{collections::HashSet, fs, io::Write as _};

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
    let stale_state = context
        .file_path
        .parent()
        .expect("state file has a parent")
        .join(format!("{}.jsonl", "a".repeat(64)));
    fs::create_dir_all(stale_state.parent().expect("stale state has a parent"))
        .expect("create state directory");
    fs::write(&stale_state, "stale").expect("write stale state");
    let state = context.start(&HashSet::from([first_key.clone()])).expect("start state");
    assert!(!stale_state.exists());
    state
        .record_passed(&second_key, &graph[&second_key], workspace.path())
        .expect("record passed task");
    fs::OpenOptions::new()
        .append(true)
        .open(&context.file_path)
        .expect("open state")
        .write_all(b"{\"run\":\"superseded\",\"project\":\"unknown\",\"task\":\"build\"}\n")
        .expect("write superseded record");
    fs::OpenOptions::new()
        .append(true)
        .open(&context.file_path)
        .expect("open state")
        .write_all(br#"{"project":"torn"#)
        .expect("write torn record");

    let completed =
        context.read_completed_tasks().expect("read state").expect("state is compatible");
    assert_eq!(completed, HashSet::from([first_key, second_key]));
    state.finish().expect("finish state");
    assert!(!context.file_path.exists());
}
