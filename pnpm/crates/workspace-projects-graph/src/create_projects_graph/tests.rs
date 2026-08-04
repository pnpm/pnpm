use crate::{
    base_project::{BaseProject, GraphProject},
    create_projects_graph::{CreateProjectsGraphOptions, Unmatched, create_projects_graph},
};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

struct TestProject {
    root_dir: PathBuf,
    name: Option<String>,
    version: Option<String>,
    peer: Vec<(String, String)>,
    dev: Vec<(String, String)>,
    optional: Vec<(String, String)>,
    prod: Vec<(String, String)>,
}

impl BaseProject for TestProject {
    fn root_dir(&self) -> &Path {
        &self.root_dir
    }
    fn manifest_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl GraphProject for TestProject {
    fn manifest_version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        let mut map: IndexMap<String, String> = IndexMap::new();
        for (name, spec) in &self.peer {
            map.insert(name.clone(), spec.clone());
        }
        if !ignore_dev_deps {
            for (name, spec) in &self.dev {
                map.insert(name.clone(), spec.clone());
            }
        }
        for (name, spec) in &self.optional {
            map.insert(name.clone(), spec.clone());
        }
        for (name, spec) in &self.prod {
            map.insert(name.clone(), spec.clone());
        }
        map.into_iter().collect()
    }
}

fn project(root: &str, name: &str, version: &str, prod: &[(&str, &str)]) -> TestProject {
    TestProject {
        root_dir: PathBuf::from(root),
        name: Some(name.to_string()),
        version: Some(version.to_string()),
        peer: Vec::new(),
        dev: Vec::new(),
        optional: Vec::new(),
        prod: prod.iter().map(|(name, spec)| (name.to_string(), spec.to_string())).collect(),
    }
}

fn edges(graph: &crate::ProjectGraph<TestProject>, key: &str) -> Vec<String> {
    graph[Path::new(key)]
        .dependencies
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn workspace_star_resolves_to_versioned_sibling() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "workspace:*")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), vec!["/ws/b".to_string()]);
    assert_eq!(edges(&result.graph, "/ws/b"), Vec::<String>::new());
    assert!(result.unmatched.is_empty());
}

#[test]
fn plain_range_resolves_to_sibling() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "^2.0.0")]),
        project("/ws/b", "b", "2.3.1", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), vec!["/ws/b".to_string()]);
    assert!(result.unmatched.is_empty());
}

#[test]
fn exact_version_string_matches_before_range() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "2.0.0")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), vec!["/ws/b".to_string()]);
}

#[test]
fn workspace_spec_links_versionless_sibling() {
    let mut versionless = project("/ws/b", "b", "0.0.0", &[]);
    versionless.version = None;
    let projects = vec![project("/ws/a", "a", "1.0.0", &[("b", "workspace:*")]), versionless];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), vec!["/ws/b".to_string()]);
}

#[test]
fn link_path_resolves_by_directory() {
    let projects = vec![
        project("/ws/packages/a", "a", "1.0.0", &[("b", "link:../b")]),
        project("/ws/packages/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/packages/a"), vec!["/ws/packages/b".to_string()]);
}

#[test]
fn path_style_workspace_spec_resolves_by_directory() {
    let projects = vec![
        project("/ws/packages/a", "a", "1.0.0", &[("b", "workspace:../b")]),
        project("/ws/packages/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/packages/a"), vec!["/ws/packages/b".to_string()]);
    assert!(result.unmatched.is_empty());
}

#[test]
fn current_dir_in_path_spec_is_collapsed() {
    let projects = vec![
        project("/ws", "a", "1.0.0", &[("b", "file:./b")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws"), vec!["/ws/b".to_string()]);
    assert!(result.unmatched.is_empty());
}

#[test]
fn windows_drive_prefixes_are_recognized_as_paths() {
    use super::has_windows_drive_prefix;
    assert!(has_windows_drive_prefix("C:/pkg"));
    assert!(has_windows_drive_prefix(r"C:\pkg"));
    assert!(!has_windows_drive_prefix("/abs/pkg"));
    assert!(!has_windows_drive_prefix("1.0.0"));
}

#[test]
fn strict_link_workspace_packages_rejects_plain_version() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "2.0.0")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let opts =
        CreateProjectsGraphOptions { ignore_dev_deps: false, link_workspace_packages: Some(false) };
    let result = create_projects_graph(projects, &opts);
    assert_eq!(edges(&result.graph, "/ws/a"), Vec::<String>::new());
    assert_eq!(
        result.unmatched,
        vec![Unmatched { pkg_name: "b".to_string(), range: "2.0.0".to_string() }],
    );
}

#[test]
fn strict_link_workspace_packages_still_links_workspace_specs() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "workspace:*")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let opts =
        CreateProjectsGraphOptions { ignore_dev_deps: false, link_workspace_packages: Some(false) };
    let result = create_projects_graph(projects, &opts);
    assert_eq!(edges(&result.graph, "/ws/a"), vec!["/ws/b".to_string()]);
    assert!(result.unmatched.is_empty());
}

#[test]
fn registry_protocols_contribute_no_edge() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "npm:other@1.0.0"), ("c", "github:o/r")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), Vec::<String>::new());
    assert!(result.unmatched.is_empty());
}

#[test]
fn ignore_dev_deps_drops_dev_only_edges() {
    let mut importer = project("/ws/a", "a", "1.0.0", &[]);
    importer.dev = vec![("b".to_string(), "workspace:*".to_string())];
    let projects = vec![importer, project("/ws/b", "b", "2.0.0", &[])];

    let with_dev = create_projects_graph(
        vec_clone(&projects),
        &CreateProjectsGraphOptions { ignore_dev_deps: false, link_workspace_packages: None },
    );
    assert_eq!(edges(&with_dev.graph, "/ws/a"), vec!["/ws/b".to_string()]);

    let without_dev = create_projects_graph(
        projects,
        &CreateProjectsGraphOptions { ignore_dev_deps: true, link_workspace_packages: None },
    );
    assert_eq!(edges(&without_dev.graph, "/ws/a"), Vec::<String>::new());
}

#[test]
fn unsatisfiable_range_is_reported_unmatched() {
    let projects = vec![
        project("/ws/a", "a", "1.0.0", &[("b", "^9.0.0")]),
        project("/ws/b", "b", "2.0.0", &[]),
    ];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), Vec::<String>::new());
    assert_eq!(
        result.unmatched,
        vec![Unmatched { pkg_name: "b".to_string(), range: "^9.0.0".to_string() }],
    );
}

#[test]
fn dependency_on_unknown_name_is_silently_skipped() {
    let projects = vec![project("/ws/a", "a", "1.0.0", &[("z", "1.0.0")])];
    let result = create_projects_graph(projects, &CreateProjectsGraphOptions::default());
    assert_eq!(edges(&result.graph, "/ws/a"), Vec::<String>::new());
    assert!(result.unmatched.is_empty());
}

fn vec_clone(projects: &[TestProject]) -> Vec<TestProject> {
    projects
        .iter()
        .map(|project| TestProject {
            root_dir: project.root_dir.clone(),
            name: project.name.clone(),
            version: project.version.clone(),
            peer: project.peer.clone(),
            dev: project.dev.clone(),
            optional: project.optional.clone(),
            prod: project.prod.clone(),
        })
        .collect()
}
