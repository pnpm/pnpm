use super::{dir, key, sequence, tasks};
use crate::{BuildTaskGraphOptions, build_task_graph};
use indexmap::IndexMap;
use std::path::Path;

#[test]
fn requested_projects_retain_transitive_dependencies_through_missing_scripts() {
    let projects = IndexMap::from([
        (dir("app"), vec![dir("middle")]),
        (dir("middle"), vec![dir("lib")]),
        (dir("lib"), vec![]),
        (dir("unrelated"), vec![]),
    ]);
    let requested = [dir("app")];
    let select_scripts = |project: &Path, task_name: &str| {
        if project == dir("middle") { vec![] } else { vec![task_name.to_string()] }
    };
    let mut options = BuildTaskGraphOptions {
        project_dependencies: &projects,
        select_scripts,
        task_name: "build",
        requested_projects: Some(&requested),
        tasks: None,
    };
    let mut graph = build_task_graph(&options);
    dbg!(&graph);
    assert_eq!(graph.len(), 3);
    assert!(graph[&key("app", "build")].requested);
    assert!(!graph[&key("middle", "build")].requested);
    assert!(!graph[&key("lib", "build")].requested);
    assert!(graph[&key("middle", "build")].scripts.is_empty());
    assert_eq!(graph[&key("app", "build")].dependencies, vec![key("middle", "build")]);
    assert_eq!(graph[&key("middle", "build")].dependencies, vec![key("lib", "build")]);

    options.requested_projects = None;
    let full_graph = build_task_graph(&options);
    dbg!(&full_graph);
    for (key, node) in &graph {
        assert_eq!(node.dependencies, full_graph[key].dependencies);
        assert_eq!(node.scripts, full_graph[key].scripts);
    }
    assert_eq!(
        sequence(&mut graph).unwrap(),
        vec![key("lib", "build"), key("middle", "build"), key("app", "build")],
    );
}

#[test]
fn requested_projects_pull_only_declared_tasks_from_other_projects() {
    let projects = IndexMap::from([
        (dir("app"), vec![dir("lib")]),
        (dir("lib"), vec![]),
        (dir("unrelated"), vec![]),
    ]);
    let settings = tasks(&[("test", Some(&["build"])), ("build", Some(&["^build"]))]);
    let graph = build_task_graph(&BuildTaskGraphOptions {
        project_dependencies: &projects,
        select_scripts: |_, name: &str| vec![name.to_string()],
        task_name: "test",
        requested_projects: Some(&[dir("app")]),
        tasks: Some(&settings),
    });
    dbg!(&graph);
    assert_eq!(
        graph.keys().cloned().collect::<Vec<_>>(),
        vec![key("app", "test"), key("app", "build"), key("lib", "build")],
    );
    assert_eq!(graph[&key("app", "test")].dependencies, vec![key("app", "build")]);
    assert_eq!(graph[&key("app", "build")].dependencies, vec![key("lib", "build")]);
    assert!(graph[&key("app", "test")].requested);
    assert!(!graph[&key("app", "build")].requested);
    assert!(!graph[&key("lib", "build")].requested);
}

#[test]
fn requested_projects_preserve_request_order_and_deduplicate_tasks() {
    let projects = IndexMap::from([(dir("app"), vec![dir("lib")]), (dir("lib"), vec![])]);
    let requested = [dir("lib"), dir("app"), dir("lib")];
    let graph = build_task_graph(&BuildTaskGraphOptions {
        project_dependencies: &projects,
        select_scripts: |_, name: &str| vec![name.to_string()],
        task_name: "build",
        requested_projects: Some(&requested),
        tasks: None,
    });
    dbg!(&graph);
    assert_eq!(
        graph.keys().cloned().collect::<Vec<_>>(),
        vec![key("lib", "build"), key("app", "build")],
    );
    assert!(graph.values().all(|node| node.requested));
    assert_eq!(graph[&key("app", "build")].dependencies, vec![key("lib", "build")]);
}

#[test]
fn empty_requested_projects_builds_no_tasks() {
    let projects = IndexMap::from([(dir("app"), vec![dir("lib")]), (dir("lib"), vec![])]);
    let graph = build_task_graph(&BuildTaskGraphOptions {
        project_dependencies: &projects,
        select_scripts: |_, _| panic!("an empty request must not select scripts"),
        task_name: "build",
        requested_projects: Some(&[]),
        tasks: None,
    });
    dbg!(&graph);
    assert!(graph.is_empty());
}
