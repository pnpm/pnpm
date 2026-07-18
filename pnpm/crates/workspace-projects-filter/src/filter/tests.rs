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

fn node_at(root: &Path, name: &str, deps: &[&Path]) -> (PathBuf, ProjectGraphNode<TestPkg>) {
    (
        root.to_path_buf(),
        ProjectGraphNode {
            package: TestPkg {
                root_dir: root.to_path_buf(),
                name: Some(name.to_string()),
                version: Some("1.0.0".to_string()),
                deps: Vec::new(),
                dev_deps: Vec::new(),
            },
            dependencies: deps.iter().map(|dep| dep.to_path_buf()).collect(),
        },
    )
}

fn filter_projects_options() -> FilterProjectsOptions {
    FilterProjectsOptions {
        prefix: PathBuf::from("/ws"),
        link_workspace_packages: None,
        use_glob_dir_filtering: false,
        workspace_dir: PathBuf::from("/ws"),
        test_pattern: Vec::new(),
        changed_files_ignore_pattern: Vec::new(),
    }
}

/// The shared project graph fixture used across the filter tests.
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
        &FilterWorkspaceProjectsOptions { use_glob_dir_filtering: true, ..Default::default() },
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

/// The `[<since>]` changed-packages selector tests, ported from
/// upstream's "select changed packages" suite. Each builds a real git
/// repository in a temp directory.
mod changed_packages {
    use super::{TestPkg, node_at};
    use crate::{
        filter::{FilterError, FilterWorkspaceProjectsOptions, filter_workspace_projects},
        parse_project_selector::ProjectSelector,
    };
    use indexmap::IndexMap;
    use pacquet_workspace_projects_graph::{GraphProject, ProjectGraph, ProjectGraphNode};
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git").args(args).current_dir(cwd).output().expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "--initial-branch=main"]);
        git(dir, &["config", "user.email", "x@y.z"]);
        git(dir, &["config", "user.name", "xyz"]);
        git(dir, &["commit", "--allow-empty", "--allow-empty-message", "-m", "", "--no-gpg-sign"]);
    }

    fn commit_all(dir: &Path) {
        git(dir, &["add", "."]);
        git(dir, &["commit", "--allow-empty-message", "-m", "", "--no-gpg-sign"]);
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().expect("file path has a parent"))
            .expect("create parent dirs");
        fs::write(path, "").expect("write file");
    }

    fn graph_of(dirs: &[&Path]) -> ProjectGraph<TestPkg> {
        let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
        for (index, dir) in dirs.iter().enumerate() {
            let (key, value) = node_at(dir, &format!("package-{index}"), &[]);
            graph.insert(key, value);
        }
        graph
    }

    fn selected(
        graph: &ProjectGraph<TestPkg>,
        selectors: &[ProjectSelector],
        opts: &FilterWorkspaceProjectsOptions,
    ) -> Vec<String> {
        filter_workspace_projects(graph, selectors, opts)
            .expect("filter should succeed")
            .selected_projects
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    fn diff_selector(since: &str) -> ProjectSelector {
        ProjectSelector { diff: Some(since.to_string()), ..Default::default() }
    }

    #[test]
    fn select_changed_packages() {
        let workspace = TempDir::new().expect("create tempdir");
        let workspace_dir = workspace.path();
        init_repo(workspace_dir);

        let pkg_1_dir = workspace_dir.join("package-1");
        touch(&pkg_1_dir.join("file1.js"));
        let pkg_2_dir = workspace_dir.join("package-2");
        touch(&pkg_2_dir.join("file2.js"));
        let pkg_3_dir = workspace_dir.join("package-3");
        fs::create_dir_all(&pkg_3_dir).expect("create package-3");
        let pkg_kor_dir = workspace_dir.join("package-kor");
        touch(&pkg_kor_dir.join("fileKor한글.js"));
        commit_all(workspace_dir);

        let pkg_20_dir = workspace_dir.join("package-20");

        let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
        for (key, value) in [
            node_at(workspace_dir, "root", &[]),
            node_at(&pkg_1_dir, "package-1", &[]),
            node_at(&pkg_2_dir, "package-2", &[]),
            node_at(&pkg_3_dir, "package-3", &[&pkg_2_dir]),
            node_at(&pkg_kor_dir, "package-kor", &[]),
            node_at(&pkg_20_dir, "package-20", &[]),
        ] {
            graph.insert(key, value);
        }

        let opts = FilterWorkspaceProjectsOptions {
            workspace_dir: workspace_dir.to_path_buf(),
            ..Default::default()
        };
        let path_of = |dir: &Path| dir.to_string_lossy().into_owned();

        assert_eq!(
            selected(&graph, &[diff_selector("HEAD~1")], &opts),
            [path_of(&pkg_1_dir), path_of(&pkg_2_dir), path_of(&pkg_kor_dir)],
        );

        assert_eq!(
            selected(
                &graph,
                &[ProjectSelector {
                    parent_dir: Some(pkg_2_dir.clone()),
                    ..diff_selector("HEAD~1")
                }],
                &opts,
            ),
            [path_of(&pkg_2_dir)],
        );

        assert_eq!(
            selected(
                &graph,
                &[ProjectSelector {
                    name_pattern: Some("package-2*".to_string()),
                    ..diff_selector("HEAD~1")
                }],
                &opts,
            ),
            [path_of(&pkg_2_dir)],
        );

        // `package-2`'s only change matches `testPattern`, so it is
        // selected itself but its dependent `package-3` is not.
        assert_eq!(
            selected(
                &graph,
                &[ProjectSelector { include_dependents: true, ..diff_selector("HEAD~1") }],
                &FilterWorkspaceProjectsOptions {
                    test_pattern: vec!["*/file2.js".to_string()],
                    ..opts.clone()
                },
            ),
            [path_of(&pkg_1_dir), path_of(&pkg_kor_dir), path_of(&pkg_2_dir)],
        );

        // Changed files matching `changedFilesIgnorePattern` don't count
        // as changes at all.
        assert_eq!(
            selected(
                &graph,
                &[diff_selector("HEAD~1")],
                &FilterWorkspaceProjectsOptions {
                    changed_files_ignore_pattern: vec!["*/file2.js".to_string()],
                    ..opts
                },
            ),
            [path_of(&pkg_1_dir), path_of(&pkg_kor_dir)],
        );
    }

    #[test]
    fn changed_catalog_entry_selects_every_project_that_references_it() {
        let workspace = TempDir::new().expect("create tempdir");
        let workspace_dir = workspace.path();
        init_repo(workspace_dir);
        fs::write(
            workspace_dir.join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\ncatalog:\n  foo: ^1.0.0\n  bar: ^1.0.0\n",
        )
        .expect("write workspace manifest");
        commit_all(workspace_dir);
        fs::write(
            workspace_dir.join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\ncatalog:\n  foo: ^2.0.0\n  bar: ^1.0.0\n",
        )
        .expect("write workspace manifest");
        commit_all(workspace_dir);

        let foo_dir = workspace_dir.join("packages/foo");
        let bar_dir = workspace_dir.join("packages/bar");
        let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
        graph.insert(
            foo_dir.clone(),
            ProjectGraphNode {
                package: TestPkg {
                    root_dir: foo_dir.clone(),
                    name: Some("foo".to_string()),
                    version: Some("1.0.0".to_string()),
                    deps: vec![("foo".to_string(), "catalog:".to_string())],
                    dev_deps: Vec::new(),
                },
                dependencies: Vec::new(),
            },
        );
        graph.insert(
            bar_dir.clone(),
            ProjectGraphNode {
                package: TestPkg {
                    root_dir: bar_dir,
                    name: Some("bar".to_string()),
                    version: Some("1.0.0".to_string()),
                    deps: vec![("bar".to_string(), "catalog:".to_string())],
                    dev_deps: Vec::new(),
                },
                dependencies: Vec::new(),
            },
        );
        let catalog_users =
            crate::get_changed_projects::collect_catalog_users(graph.values().map(|node| {
                (node.package.root_dir.clone(), node.package.merged_dependencies(false))
            }));

        assert_eq!(
            selected(
                &graph,
                &[diff_selector("HEAD~1")],
                &FilterWorkspaceProjectsOptions {
                    workspace_dir: workspace_dir.to_path_buf(),
                    catalog_users,
                    ..Default::default()
                },
            ),
            [foo_dir.to_string_lossy().into_owned()],
        );
    }

    #[test]
    fn select_changed_packages_under_git_worktree() {
        let main_repo = TempDir::new().expect("create tempdir");
        let main_repo_dir = main_repo.path();
        init_repo(main_repo_dir);
        touch(&main_repo_dir.join("package-a").join("file.js"));
        touch(&main_repo_dir.join("package-b").join("file.js"));
        touch(&main_repo_dir.join("package-c").join("file.js"));
        commit_all(main_repo_dir);

        let worktree_parent = TempDir::new().expect("create tempdir");
        let worktree_dir = worktree_parent.path().join("worktree");
        git(
            main_repo_dir,
            &["worktree", "add", "-b", "worktree-branch", &worktree_dir.to_string_lossy(), "main"],
        );

        let pkg_a_dir = worktree_dir.join("package-a");
        touch(&pkg_a_dir.join("new-file.js"));
        commit_all(&worktree_dir);

        let graph = graph_of(&[
            &worktree_dir,
            &pkg_a_dir,
            &worktree_dir.join("package-b"),
            &worktree_dir.join("package-c"),
        ]);

        assert_eq!(
            selected(
                &graph,
                &[diff_selector("HEAD~1")],
                &FilterWorkspaceProjectsOptions {
                    workspace_dir: worktree_dir,
                    ..Default::default()
                },
            ),
            [pkg_a_dir.to_string_lossy().into_owned()],
        );
    }

    #[test]
    fn selection_fails_for_nonexistent_diff_branch() {
        let workspace = TempDir::new().expect("create tempdir");
        init_repo(workspace.path());
        let graph = graph_of(&[&workspace.path().join("package-a")]);

        let error = filter_workspace_projects(
            &graph,
            &[diff_selector("branch-does-no-exist")],
            &FilterWorkspaceProjectsOptions {
                workspace_dir: workspace.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap_err();

        dbg!(&error);
        assert!(matches!(error, FilterError::FilterChanged { .. }));
        assert_eq!(
            error.to_string(),
            "Filtering by changed packages failed. fatal: bad revision 'branch-does-no-exist'",
        );
    }

    /// An option-like `<since>` must not be parsed as a git option:
    /// without `--end-of-options`, `[--output=<path>]` would make
    /// `git diff` write its output to an arbitrary file and exit 0.
    #[test]
    fn option_like_diff_ref_is_rejected_as_bad_revision() {
        let workspace = TempDir::new().expect("create tempdir");
        init_repo(workspace.path());
        touch(&workspace.path().join("package-a").join("file.js"));
        commit_all(workspace.path());
        let graph = graph_of(&[&workspace.path().join("package-a")]);

        let evil_output = workspace.path().join("evil.txt");
        let error = filter_workspace_projects(
            &graph,
            &[diff_selector(&format!("--output={}", evil_output.to_string_lossy()))],
            &FilterWorkspaceProjectsOptions {
                workspace_dir: workspace.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap_err();

        dbg!(&error);
        assert!(matches!(error, FilterError::FilterChanged { .. }));
        assert!(
            error.to_string().contains("bad revision"),
            "git must treat the option-like ref as a revision, got: {error}",
        );
        assert!(!evil_output.exists(), "the ref must not be honored as a git option");
    }

    /// A worktree checked out *inside* another repository's tree still
    /// resolves diff paths against the worktree root: the nearest
    /// `.git` entry is the worktree's `.git` file, even though an
    /// ancestor has a `.git` directory.
    #[test]
    fn select_changed_packages_under_git_worktree_nested_in_main_repo() {
        let main_repo = TempDir::new().expect("create tempdir");
        let main_repo_dir = main_repo.path();
        init_repo(main_repo_dir);
        touch(&main_repo_dir.join("package-a").join("file.js"));
        touch(&main_repo_dir.join("package-b").join("file.js"));
        commit_all(main_repo_dir);

        let worktree_dir = main_repo_dir.join("worktrees").join("feature");
        git(
            main_repo_dir,
            &["worktree", "add", "-b", "feature", &worktree_dir.to_string_lossy(), "main"],
        );

        let pkg_a_dir = worktree_dir.join("package-a");
        touch(&pkg_a_dir.join("new-file.js"));
        commit_all(&worktree_dir);

        let graph = graph_of(&[&worktree_dir, &pkg_a_dir, &worktree_dir.join("package-b")]);

        assert_eq!(
            selected(
                &graph,
                &[diff_selector("HEAD~1")],
                &FilterWorkspaceProjectsOptions {
                    workspace_dir: worktree_dir,
                    ..Default::default()
                },
            ),
            [pkg_a_dir.to_string_lossy().into_owned()],
        );
    }
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
        &filter_projects_options(),
    )
    .unwrap();
    let dirs: Vec<String> =
        result.selected_projects.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    assert_eq!(dirs, ["/ws/a", "/ws/b"]);
}

#[test]
fn filter_projects_empty_filter_selects_everything() {
    let projects = vec![graph_project("/ws/a", "a", &[]), graph_project("/ws/b", "b", &[])];
    let result = filter_projects(projects, &[], &filter_projects_options()).unwrap();
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
