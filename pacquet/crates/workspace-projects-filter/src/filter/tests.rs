use crate::{
    filter::{
        FilterError, FilterProjectsOptions, FilterWorkspaceProjectsOptions, WorkspaceFilter,
        filter_projects, filter_workspace_projects,
    },
    parse_project_selector::ProjectSelector,
};
use indexmap::IndexMap;
use pacquet_workspace_projects_graph::{BaseProject, GraphProject, ProjectGraph, ProjectGraphNode};
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct TestPkg {
    root_dir: PathBuf,
    name: Option<String>,
    version: Option<String>,
    deps: Vec<(String, String)>,
    dev_deps: Vec<(String, String)>,
}

impl BaseProject for TestPkg {
    fn root_dir(&self) -> &Path {
        &self.root_dir
    }
    fn manifest_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl GraphProject for TestPkg {
    fn manifest_version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        let mut merged = self.deps.clone();
        if !ignore_dev_deps {
            merged.extend(self.dev_deps.iter().cloned());
        }
        merged
    }
}

fn node(root: &str, name: &str, deps: &[&str]) -> (PathBuf, ProjectGraphNode<TestPkg>) {
    (
        PathBuf::from(root),
        ProjectGraphNode {
            package: TestPkg {
                root_dir: PathBuf::from(root),
                name: Some(name.to_string()),
                version: Some("1.0.0".to_string()),
                deps: Vec::new(),
                dev_deps: Vec::new(),
            },
            dependencies: deps.iter().map(PathBuf::from).collect(),
        },
    )
}

/// The shared fixture from upstream's
/// [`projects-filter` test](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/test/index.ts#L22-L124).
fn projects_graph() -> ProjectGraph<TestPkg> {
    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    for (key, value) in [
        node("/packages/project-0", "project-0", &["/packages/project-1", "/project-5"]),
        node("/packages/project-1", "project-1", &["/project-2", "/project-4"]),
        node("/project-2", "project-2", &[]),
        node("/project-3", "project-3", &[]),
        node("/project-4", "project-4", &[]),
        node("/project-5", "project-5", &[]),
        node("/project-5/packages/project-6", "project-6", &[]),
    ] {
        graph.insert(key, value);
    }
    graph
}

fn selector(name_pattern: Option<&str>) -> ProjectSelector {
    ProjectSelector { name_pattern: name_pattern.map(str::to_string), ..Default::default() }
}

fn selected(graph: &ProjectGraph<TestPkg>, selectors: &[ProjectSelector]) -> Vec<String> {
    filter_workspace_projects(graph, selectors, &FilterWorkspaceProjectsOptions::default())
        .expect("filter should succeed")
        .selected_projects
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

fn selected_with_glob(graph: &ProjectGraph<TestPkg>, selectors: &[ProjectSelector]) -> Vec<String> {
    filter_workspace_projects(
        graph,
        selectors,
        &FilterWorkspaceProjectsOptions { use_glob_dir_filtering: true },
    )
    .expect("filter should succeed")
    .selected_projects
    .iter()
    .map(|path| path.to_string_lossy().into_owned())
    .collect()
}

#[test]
fn select_only_package_dependencies() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector {
            exclude_self: true,
            include_dependencies: true,
            ..selector(Some("project-1"))
        }],
    );
    assert_eq!(result, ["/project-2", "/project-4"]);
}

#[test]
fn select_package_with_dependencies() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector { include_dependencies: true, ..selector(Some("project-1")) }],
    );
    assert_eq!(result, ["/packages/project-1", "/project-2", "/project-4"]);
}

#[test]
fn shared_dependency_in_diamond_is_walked_once() {
    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    for (key, value) in [
        node("/top", "top", &["/left", "/right"]),
        node("/left", "left", &["/shared"]),
        node("/right", "right", &["/shared"]),
        node("/shared", "shared", &[]),
    ] {
        graph.insert(key, value);
    }
    let result = selected(
        &graph,
        &[ProjectSelector { include_dependencies: true, ..selector(Some("top")) }],
    );
    assert_eq!(result, ["/top", "/left", "/shared", "/right"]);
}

#[test]
fn select_package_with_dependencies_and_dependents() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector {
            exclude_self: true,
            include_dependencies: true,
            include_dependents: true,
            ..selector(Some("project-1"))
        }],
    );
    assert_eq!(
        result,
        ["/project-2", "/project-4", "/packages/project-0", "/packages/project-1", "/project-5"],
    );
}

#[test]
fn select_package_with_dependents() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector { include_dependents: true, ..selector(Some("project-2")) }],
    );
    assert_eq!(result, ["/project-2", "/packages/project-1", "/packages/project-0"]);
}

#[test]
fn select_dependents_excluding_self() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector {
            exclude_self: true,
            include_dependents: true,
            ..selector(Some("project-2"))
        }],
    );
    assert_eq!(result, ["/packages/project-1", "/packages/project-0"]);
}

#[test]
fn two_selectors_dependencies_and_dependents() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[
            ProjectSelector {
                exclude_self: true,
                include_dependents: true,
                ..selector(Some("project-2"))
            },
            ProjectSelector {
                exclude_self: true,
                include_dependencies: true,
                ..selector(Some("project-1"))
            },
        ],
    );
    assert_eq!(result, ["/project-2", "/project-4", "/packages/project-1", "/packages/project-0"]);
}

#[test]
fn select_just_a_package_by_name() {
    let graph = projects_graph();
    let result = selected(&graph, &[selector(Some("project-2"))]);
    assert_eq!(result, ["/project-2"]);
}

#[test]
fn select_package_without_specifying_scope() {
    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    graph.insert(PathBuf::from("/packages/bar"), node("/packages/bar", "@foo/bar", &[]).1);
    let result = selected(&graph, &[selector(Some("bar"))]);
    assert_eq!(result, ["/packages/bar"]);
}

#[test]
fn scoped_package_with_same_name_picks_exact_match() {
    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    graph
        .insert(PathBuf::from("/packages/@foo/bar"), node("/packages/@foo/bar", "@foo/bar", &[]).1);
    graph.insert(PathBuf::from("/packages/bar"), node("/packages/bar", "bar", &[]).1);
    let result = selected(&graph, &[selector(Some("bar"))]);
    assert_eq!(result, ["/packages/bar"]);
}

#[test]
fn two_scoped_packages_matching_name_select_none() {
    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    graph
        .insert(PathBuf::from("/packages/@foo/bar"), node("/packages/@foo/bar", "@foo/bar", &[]).1);
    graph.insert(
        PathBuf::from("/packages/@types/bar"),
        node("/packages/@types/bar", "@types/bar", &[]).1,
    );
    let result = selected(&graph, &[selector(Some("bar"))]);
    assert!(result.is_empty(), "ambiguous scoped match should select nothing, got {result:?}");
}

#[test]
fn select_by_parent_dir() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector { parent_dir: Some(PathBuf::from("/packages")), ..Default::default() }],
    );
    assert_eq!(result, ["/packages/project-0", "/packages/project-1"]);
}

#[test]
fn select_by_parent_dir_using_glob() {
    let graph = projects_graph();
    let result = selected_with_glob(
        &graph,
        &[ProjectSelector { parent_dir: Some(PathBuf::from("/packages/*")), ..Default::default() }],
    );
    assert_eq!(result, ["/packages/project-0", "/packages/project-1"]);
}

#[test]
fn select_by_parent_dir_using_globstar() {
    let graph = projects_graph();
    let result = selected_with_glob(
        &graph,
        &[ProjectSelector {
            parent_dir: Some(PathBuf::from("/project-5/**")),
            ..Default::default()
        }],
    );
    assert_eq!(result, ["/project-5", "/project-5/packages/project-6"]);
}

#[test]
fn select_by_parent_dir_with_no_glob() {
    let graph = projects_graph();
    let result = selected_with_glob(
        &graph,
        &[ProjectSelector { parent_dir: Some(PathBuf::from("/project-5")), ..Default::default() }],
    );
    assert_eq!(result, ["/project-5"]);
}

#[test]
fn returns_unmatched_filters() {
    let graph = projects_graph();
    let result = filter_workspace_projects(
        &graph,
        &[ProjectSelector {
            exclude_self: true,
            include_dependencies: true,
            ..selector(Some("project-7"))
        }],
        &FilterWorkspaceProjectsOptions::default(),
    )
    .unwrap();
    assert!(result.selected_projects.is_empty());
    assert_eq!(result.unmatched_filters, ["project-7"]);
}

#[test]
fn select_all_packages_except_one() {
    let graph = projects_graph();
    let result =
        selected(&graph, &[ProjectSelector { exclude: true, ..selector(Some("project-1")) }]);
    let expected: Vec<String> = projects_graph()
        .keys()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| path != "/packages/project-1")
        .collect();
    assert_eq!(result, expected);
}

#[test]
fn select_by_parent_dir_and_exclude_by_pattern() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[
            ProjectSelector { parent_dir: Some(PathBuf::from("/packages")), ..Default::default() },
            ProjectSelector { exclude: true, ..selector(Some("*-1")) },
        ],
    );
    assert_eq!(result, ["/packages/project-0"]);
}

#[test]
fn select_by_parent_dir_glob_and_exclude_by_pattern() {
    let graph = projects_graph();
    let result = selected_with_glob(
        &graph,
        &[
            ProjectSelector {
                parent_dir: Some(PathBuf::from("/packages/*")),
                ..Default::default()
            },
            ProjectSelector { exclude: true, ..selector(Some("*-1")) },
        ],
    );
    assert_eq!(result, ["/packages/project-0"]);
}

#[test]
fn select_by_parent_dir_then_name_pattern() {
    let graph = projects_graph();
    let result = selected(
        &graph,
        &[ProjectSelector {
            parent_dir: Some(PathBuf::from("/packages")),
            name_pattern: Some("*-0".to_string()),
            ..Default::default()
        }],
    );
    assert_eq!(result, ["/packages/project-0"]);
}

#[test]
fn selector_without_name_dir_or_diff_is_unsupported() {
    let graph = projects_graph();
    let error = filter_workspace_projects(
        &graph,
        &[ProjectSelector { include_dependencies: true, ..Default::default() }],
        &FilterWorkspaceProjectsOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error, FilterError::UnsupportedSelector { .. }));
}

#[test]
fn is_subdir_contract() {
    use super::is_subdir;
    assert!(!is_subdir(Path::new("/abs/parent"), Path::new("relative/child")));
    assert!(is_subdir(Path::new("/abs"), Path::new("/abs/child")));
    assert!(!is_subdir(Path::new("/abs"), Path::new("/abs")));
}

#[test]
fn diff_selector_is_unsupported() {
    let graph = projects_graph();
    let error = filter_workspace_projects(
        &graph,
        &[ProjectSelector { diff: Some("main".to_string()), ..Default::default() }],
        &FilterWorkspaceProjectsOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error, FilterError::UnsupportedDiffSelector));
}

fn graph_project(root: &str, name: &str, deps: &[(&str, &str)]) -> TestPkg {
    TestPkg {
        root_dir: PathBuf::from(root),
        name: Some(name.to_string()),
        version: Some("1.0.0".to_string()),
        deps: deps.iter().map(|(name, spec)| (name.to_string(), spec.to_string())).collect(),
        dev_deps: Vec::new(),
    }
}

fn project_dirs(result: &crate::filter::FilteredProjects) -> Vec<String> {
    result.selected_projects.iter().map(|path| path.to_string_lossy().into_owned()).collect()
}

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
        &FilterProjectsOptions {
            prefix: PathBuf::from("/ws"),
            link_workspace_packages: None,
            use_glob_dir_filtering: false,
        },
    )
    .unwrap();
    let dirs: Vec<String> =
        result.selected_projects.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    assert_eq!(dirs, ["/ws/a", "/ws/b"]);
}

#[test]
fn filter_projects_empty_filter_selects_everything() {
    let projects = vec![graph_project("/ws/a", "a", &[]), graph_project("/ws/b", "b", &[])];
    let result = filter_projects(
        projects,
        &[],
        &FilterProjectsOptions {
            prefix: PathBuf::from("/ws"),
            link_workspace_packages: None,
            use_glob_dir_filtering: false,
        },
    )
    .unwrap();
    let dirs: Vec<String> =
        result.selected_projects.iter().map(|path| path.to_string_lossy().into_owned()).collect();
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
    let opts = FilterProjectsOptions {
        prefix: PathBuf::from("/ws"),
        link_workspace_packages: None,
        use_glob_dir_filtering: false,
    };

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
        &FilterProjectsOptions {
            prefix: PathBuf::from("/ws"),
            link_workspace_packages: None,
            use_glob_dir_filtering: false,
        },
    )
    .unwrap();
    assert_eq!(project_dirs(&result), ["/ws/b", "/ws/a"]);
}

#[test]
fn path_selector_with_no_match_is_reported_unmatched() {
    let graph = projects_graph();
    let result = filter_workspace_projects(
        &graph,
        &[ProjectSelector {
            parent_dir: Some(PathBuf::from("/does-not-exist")),
            ..Default::default()
        }],
        &FilterWorkspaceProjectsOptions::default(),
    )
    .unwrap();
    assert!(result.selected_projects.is_empty());
    assert_eq!(result.unmatched_filters, ["/does-not-exist"]);
}

/// Ports of upstream `projects-filter` cases that require the git-diff
/// changed-packages selector (`[<since>]`). Pacquet parses the selector
/// but has not ported git-diff project selection, so these are stubbed
/// through [`pacquet_testing_utils::allow_known_failure`] until that
/// lands. The selector-rejection path itself is covered by
/// [`super::diff_selector_is_unsupported`].
mod known_failures {
    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn git_diff_selection_unimplemented() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet has not ported git-diff project selection, so `[<since>]` \
             changed-package filter selectors cannot be evaluated. \
             `parse_project_selector` fills the `diff` field, but \
             `filter_workspace_projects` rejects it with \
             `FilterError::UnsupportedDiffSelector`.",
        ))
    }

    /// Upstream: [`index.ts:348` "select changed packages"](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/test/index.ts#L348).
    #[test]
    fn select_changed_packages() {
        allow_known_failure!(git_diff_selection_unimplemented());
    }

    /// Upstream: [`index.ts:480` "select changed packages when operating under a git worktree"](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/test/index.ts#L480).
    #[test]
    fn select_changed_packages_under_git_worktree() {
        allow_known_failure!(git_diff_selection_unimplemented());
    }

    /// Upstream: [`index.ts:553` "selection should fail when diffing to a branch that does not exist"](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/test/index.ts#L553).
    #[test]
    fn selection_fails_for_nonexistent_diff_branch() {
        allow_known_failure!(git_diff_selection_unimplemented());
    }
}
