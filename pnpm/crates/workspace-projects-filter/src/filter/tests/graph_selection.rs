use super::{
    TestPkg,
    WorkspaceFilter,
    filter_projects,
    filter_projects_options,
    graph_project,
    project_dirs,
};
use std::path::PathBuf;

#[test]
fn filter_projects_builds_graph_and_follows_dependencies() {
    let projects = vec![
        graph_project("/ws/a", "a", &[("b", "workspace:*")]),
        graph_project("/ws/b", "b", &[]),
        graph_project("/ws/c", "c", &[]),
    ];
    let result = filter_projects(
        projects,
        &[WorkspaceFilter { filter: "a...".to_string(), follow_prod_deps_only: false }],
        &filter_projects_options(),
    )
    .unwrap();
    let dirs: Vec<String> = result.selected_projects
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(dirs, ["/ws/a", "/ws/b"]);
}

#[test]
fn filter_projects_empty_filter_selects_everything() {
    let projects = vec![graph_project("/ws/a", "a", &[]), graph_project("/ws/b", "b", &[])];
    let result = filter_projects(projects, &[], &filter_projects_options()).unwrap();
    let dirs: Vec<String> = result.selected_projects
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(dirs, ["/ws/a", "/ws/b"]);
}

#[test]
fn filter_prod_follows_production_deps_only() {
    let make_projects = || {
        vec![
            TestPkg {
                root_dir: PathBuf::from("/ws/a"),
                name: Some("a".to_string()),
                version: Some("1.0.0".to_string()),
                deps: Vec::new(),
                dev_deps: vec![("b".to_string(), "workspace:*".to_string())],
            },
            graph_project("/ws/b", "b", &[]),
        ]
    };
    let opts = filter_projects_options();

    let prod = filter_projects(
        make_projects(),
        &[WorkspaceFilter { filter: "a...".to_string(), follow_prod_deps_only: true }],
        &opts,
    )
    .unwrap();
    assert_eq!(project_dirs(&prod), ["/ws/a"]);

    let all = filter_projects(
        make_projects(),
        &[WorkspaceFilter { filter: "a...".to_string(), follow_prod_deps_only: false }],
        &opts,
    )
    .unwrap();
    assert_eq!(project_dirs(&all), ["/ws/a", "/ws/b"]);
}

#[test]
fn filter_projects_unions_prod_selection_before_all_selection() {
    let projects = vec![graph_project("/ws/a", "a", &[]), graph_project("/ws/b", "b", &[])];
    let result = filter_projects(
        projects,
        &[
            WorkspaceFilter { filter: "b".to_string(), follow_prod_deps_only: true },
            WorkspaceFilter { filter: "a".to_string(), follow_prod_deps_only: false },
        ],
        &filter_projects_options(),
    )
    .unwrap();
    assert_eq!(project_dirs(&result), ["/ws/b", "/ws/a"]);
}
