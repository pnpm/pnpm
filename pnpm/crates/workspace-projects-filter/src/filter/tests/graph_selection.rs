use super::{
    FilterProjectsOptions, TestPkg, WorkspaceFilter, filter_projects, filter_projects_options,
    graph_project, project_dirs,
};
use pnpm_catalogs_types::Catalogs;
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

#[test]
fn filter_projects_follows_catalog_dependencies() {
    let projects =
        vec![graph_project("/ws/a", "a", &[("b", "catalog:")]), graph_project("/ws/b", "b", &[])];
    let opts = FilterProjectsOptions {
        catalogs: Some(Catalogs::from([(
            "default".to_string(),
            [("b".to_string(), "workspace:*".to_string())].into(),
        )])),
        ..filter_projects_options()
    };
    for follow_prod_deps_only in [false, true] {
        let result = filter_projects(
            projects.clone(),
            &[WorkspaceFilter { filter: "a...".to_string(), follow_prod_deps_only }],
            &opts,
        )
        .unwrap();
        assert_eq!(project_dirs(&result), ["/ws/a", "/ws/b"]);
    }
}

#[test]
fn filter_prod_inclusion_with_regular_exclusion_does_not_seed_workspace() {
    let projects = vec![
        graph_project("/ws", "ws-root", &[]),
        TestPkg {
            root_dir: PathBuf::from("/ws/packages/app"),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            deps: vec![("lib-a".to_string(), "workspace:*".to_string())],
            dev_deps: vec![("lib-dev".to_string(), "workspace:*".to_string())],
        },
        graph_project("/ws/packages/lib-a", "lib-a", &[]),
        graph_project("/ws/packages/lib-dev", "lib-dev", &[]),
        graph_project("/ws/packages/unrelated", "unrelated", &[]),
    ];
    let result = filter_projects(
        projects,
        &[
            WorkspaceFilter { filter: "app...".to_string(), follow_prod_deps_only: true },
            WorkspaceFilter { filter: "!lib-a".to_string(), follow_prod_deps_only: false },
        ],
        &filter_projects_options(),
    )
    .unwrap();
    assert_eq!(project_dirs(&result), ["/ws/packages/app"]);
}

#[test]
fn regular_inclusion_with_filter_prod_exclusion_subtracts_prod_closure_only() {
    let projects = vec![
        graph_project("/ws", "ws-root", &[]),
        graph_project("/ws/app", "app", &[("lib-a", "workspace:*")]),
        TestPkg {
            root_dir: PathBuf::from("/ws/lib-a"),
            name: Some("lib-a".to_string()),
            version: Some("1.0.0".to_string()),
            deps: vec![("prod-leaf".to_string(), "workspace:*".to_string())],
            dev_deps: vec![("dev-leaf".to_string(), "workspace:*".to_string())],
        },
        graph_project("/ws/prod-leaf", "prod-leaf", &[]),
        graph_project("/ws/dev-leaf", "dev-leaf", &[]),
        graph_project("/ws/unrelated", "unrelated", &[]),
    ];
    let result = filter_projects(
        projects,
        &[
            WorkspaceFilter { filter: "app...".to_string(), follow_prod_deps_only: false },
            WorkspaceFilter { filter: "!lib-a...".to_string(), follow_prod_deps_only: true },
        ],
        &filter_projects_options(),
    )
    .unwrap();
    assert_eq!(project_dirs(&result), ["/ws/app", "/ws/dev-leaf"]);
}

#[test]
fn leading_exclusion_followed_by_filter_prod_inclusion_reincludes_prod_closure() {
    let projects = vec![
        graph_project("/ws", "ws-root", &[]),
        TestPkg {
            root_dir: PathBuf::from("/ws/packages/app"),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            deps: vec![("lib-a".to_string(), "workspace:*".to_string())],
            dev_deps: vec![("lib-dev".to_string(), "workspace:*".to_string())],
        },
        graph_project("/ws/packages/lib-a", "lib-a", &[]),
        graph_project("/ws/packages/lib-dev", "lib-dev", &[]),
        graph_project("/ws/packages/unrelated", "unrelated", &[]),
    ];
    let opts = FilterProjectsOptions { use_glob_dir_filtering: true, ..filter_projects_options() };
    let result = filter_projects(
        projects,
        &[
            WorkspaceFilter { filter: "!{packages/*}".to_string(), follow_prod_deps_only: false },
            WorkspaceFilter { filter: "app...".to_string(), follow_prod_deps_only: true },
        ],
        &opts,
    )
    .unwrap();
    assert_eq!(project_dirs(&result), ["/ws/packages/app", "/ws/packages/lib-a", "/ws"]);
}
