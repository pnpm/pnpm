use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    path::PathBuf,
};

use pnpm_lockfile::Lockfile;
use pretty_assertions::assert_eq;

use super::{
    super::{ProjectInfo, ProjectPathKey, create_file_url_key},
    ConvertCtx, NamedProjectRoot, approve_legacy_workspace_project_builds,
    approve_workspace_project_builds,
};

struct Workspace {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    deploy_dir: PathBuf,
    projects_by_path: HashMap<ProjectPathKey, ProjectInfo>,
}

impl Workspace {
    fn with_project(name: &str) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("workspace");
        let project_root = root.join("packages").join(name);
        let mut projects_by_path = HashMap::new();
        projects_by_path.insert(
            ProjectPathKey::new(&project_root),
            ProjectInfo {
                root_dir: project_root,
                linked_publish_dir: None,
                name: Some(name.to_string()),
                has_bin: false,
                peer_dependencies: Vec::new(),
                peer_blocks: (None, None),
                declared_dependencies: HashSet::default(),
            },
        );
        let deploy_dir = root.join("deploy");
        Self { _tmp: tmp, root, deploy_dir, projects_by_path }
    }

    /// A workspace whose `lib` project dependents link from
    /// `linked_publish_dir`, relative to the workspace root. The directory
    /// is created.
    fn with_linked_publish_dir(linked_publish_dir: &str) -> Self {
        let mut workspace = Self::with_project("lib");
        let linked_publish_dir = workspace.root.join(linked_publish_dir);
        std::fs::create_dir_all(&linked_publish_dir).expect("create the publish directory");
        let project_root = workspace.root.join("packages/lib");
        std::fs::create_dir_all(&project_root).expect("create the project root");
        workspace.projects_by_path
            .get_mut(&ProjectPathKey::new(&project_root))
            .expect("the lib project")
            .linked_publish_dir = Some(linked_publish_dir);
        workspace
    }

    /// Approves the deployed package installed from `relative_path`.
    fn approve_package_at(&self, relative_path: &str) -> HashMap<String, bool> {
        self.approve_package_at_with(&[], relative_path)
    }

    /// Approves the deployed package installed from `relative_path`, given
    /// `allow_builds`.
    fn approve_package_at_with(
        &self,
        allow_builds: &[(&str, bool)],
        relative_path: &str,
    ) -> HashMap<String, bool> {
        let key = self.file_key(relative_path);
        let lockfile = Self::lockfile(&[(&key, &format!("../{relative_path}"))]);
        self.approve(allow_builds, &lockfile)
    }

    /// The key the deploy generates for the directory at `relative_path`
    /// under the workspace root.
    fn file_key(&self, relative_path: &str) -> String {
        create_file_url_key(&self.root.join(relative_path), "", &self.projects_by_path, None)
            .expect("create file URL key")
            .to_string()
    }

    /// A deployed lockfile with one directory package per `(key, directory)`
    /// pair, the directory relative to the deploy dir.
    fn lockfile(packages: &[(&str, &str)]) -> Lockfile {
        let mut yaml = String::from("lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n");
        for (key, directory) in packages {
            writeln!(
                yaml,
                "  '{key}':\n    resolution: {{ type: directory, directory: {directory} }}",
            )
            .expect("write to a String");
        }
        serde_saphyr::from_str(&yaml).expect("parse lockfile")
    }

    fn approve(&self, allow_builds: &[(&str, bool)], lockfile: &Lockfile) -> HashMap<String, bool> {
        let mut allow_builds = allow_builds
            .iter()
            .map(|(key, value)| (key.to_string(), *value))
            .collect();
        let deployed_project_root = self.root.join("packages/app");
        let ctx = ConvertCtx {
            projects_by_path: &self.projects_by_path,
            deploy_dir: &self.deploy_dir,
            lockfile_dir: &self.root,
            deployed_project_root: &deployed_project_root,
        };
        approve_workspace_project_builds(&mut allow_builds, lockfile, &ctx)
            .expect("approve workspace project builds");
        allow_builds
    }
}

fn approvals(entries: &[(&str, bool)]) -> HashMap<String, bool> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), *value))
        .collect()
}

#[test]
fn approves_a_workspace_project_by_its_dependency_path() {
    let workspace = Workspace::with_project("lib");
    let lib = workspace.file_key("packages/lib");
    let lockfile = Workspace::lockfile(&[(&lib, "../packages/lib")]);

    let allow_builds = workspace.approve(&[("lib", true)], &lockfile);

    assert_eq!(allow_builds, approvals(&[("lib", true), (&lib, true)]));
}

#[test]
fn does_not_approve_a_directory_that_is_not_a_workspace_project() {
    let workspace = Workspace::with_project("lib");
    let vendored = workspace.file_key("vendor/lib");
    let lockfile = Workspace::lockfile(&[(&vendored, "../vendor/lib")]);

    assert_eq!(workspace.approve(&[], &lockfile), approvals(&[]));
}

#[test]
fn does_not_approve_a_project_key_that_resolves_to_another_directory() {
    let workspace = Workspace::with_project("lib");
    let lib = workspace.file_key("packages/lib");
    let lockfile = Workspace::lockfile(&[(&lib, "../vendor/lib")]);

    assert_eq!(workspace.approve(&[], &lockfile), approvals(&[]));
}

#[test]
fn keeps_a_denial_of_a_workspace_project() {
    let workspace = Workspace::with_project("lib");
    let lib = workspace.file_key("packages/lib");
    let lockfile = Workspace::lockfile(&[(&lib, "../packages/lib")]);

    let allow_builds = workspace.approve(&[("lib", false)], &lockfile);

    assert_eq!(allow_builds, approvals(&[("lib", false)]));
}

#[test]
fn keeps_a_denial_of_the_key_the_source_workspace_gives_a_project() {
    let workspace = Workspace::with_project("lib");
    let lib = workspace.file_key("packages/lib");
    let lockfile = Workspace::lockfile(&[(&lib, "../packages/lib")]);

    let allow_builds = workspace.approve(&[("lib@file:packages/lib", false)], &lockfile);

    assert_eq!(allow_builds, approvals(&[("lib@file:packages/lib", false)]));
}

#[test]
fn keeps_a_source_workspace_denial_of_a_legacy_workspace_project() {
    let workspace = Workspace::with_project("lib");
    let workspace_projects = [NamedProjectRoot {
        name: "lib".to_string(),
        root_dir: workspace.root.join("packages/lib"),
        linked_publish_dir: None,
    }];
    let mut allow_builds = approvals(&[("lib@file:packages/lib", false)]);

    approve_legacy_workspace_project_builds(
        &mut allow_builds,
        &workspace_projects,
        &workspace.deploy_dir,
        &workspace.root,
    )
    .expect("approve legacy workspace project builds");

    assert_eq!(allow_builds, approvals(&[("lib@file:packages/lib", false)]));
}

#[test]
fn approves_a_legacy_workspace_project_relative_to_the_lockfile_dir() {
    let workspace = Workspace::with_project("lib");
    let workspace_projects = [
        NamedProjectRoot {
            name: "lib".to_string(),
            root_dir: workspace.root.join("packages/lib"),
            linked_publish_dir: None,
        },
        NamedProjectRoot {
            name: "denied".to_string(),
            root_dir: workspace.root.join("packages/denied"),
            linked_publish_dir: None,
        },
    ];
    let mut allow_builds = approvals(&[("denied", false)]);

    approve_legacy_workspace_project_builds(
        &mut allow_builds,
        &workspace_projects,
        &workspace.root,
        &workspace.root,
    )
    .expect("approve legacy workspace project builds");

    assert_eq!(allow_builds, approvals(&[("denied", false), ("lib@file:packages/lib", true)]));
}

#[cfg(unix)]
#[test]
fn does_not_approve_a_directory_whose_name_only_prints_like_a_project_root() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let workspace = Workspace::with_project("lib");
    let project_root = workspace.root.join(OsStr::from_bytes(b"lib\xff"));
    let mut projects_by_path = HashMap::new();
    projects_by_path.insert(
        ProjectPathKey::new(&project_root),
        ProjectInfo {
            root_dir: project_root,
            linked_publish_dir: None,
            name: Some("lib".to_string()),
            has_bin: false,
            peer_dependencies: Vec::new(),
            peer_blocks: (None, None),
            declared_dependencies: HashSet::default(),
        },
    );
    let workspace = Workspace { projects_by_path, ..workspace };
    let look_alike = workspace.file_key("lib\u{FFFD}");
    let lockfile = Workspace::lockfile(&[(&look_alike, "../lib\u{FFFD}")]);

    assert_eq!(workspace.approve(&[], &lockfile), approvals(&[]));
}

#[test]
fn approves_the_publish_directory_of_a_workspace_project() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/dist");
    let lib = workspace.file_key("packages/lib/dist");

    assert_eq!(workspace.approve_package_at("packages/lib/dist"), approvals(&[(&lib, true)]));
}

#[test]
fn does_not_approve_a_publish_directory_outside_the_project_root() {
    let workspace = Workspace::with_linked_publish_dir("vendor/lib");

    assert_eq!(workspace.approve_package_at("vendor/lib"), approvals(&[]));
}

#[test]
fn does_not_approve_a_publish_directory_inside_node_modules() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/node_modules/dep");

    assert_eq!(workspace.approve_package_at("packages/lib/node_modules/dep"), approvals(&[]));
}

#[test]
fn does_not_approve_a_publish_directory_inside_node_modules_spelled_in_another_case() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/NODE_MODULES/dep");

    assert_eq!(workspace.approve_package_at("packages/lib/NODE_MODULES/dep"), approvals(&[]));
}

#[test]
fn keeps_a_source_workspace_denial_of_the_project_root_for_its_publish_directory() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/dist");

    assert_eq!(
        workspace.approve_package_at_with(&[("lib@file:packages/lib", false)], "packages/lib/dist"),
        approvals(&[("lib@file:packages/lib", false)]),
    );
}

#[cfg(unix)]
#[test]
fn does_not_approve_a_publish_directory_reached_through_a_symlink() {
    let workspace = Workspace::with_project("lib");
    let vendor = workspace.root.join("vendor");
    std::fs::create_dir_all(&vendor).expect("create the symlink target");
    std::fs::create_dir_all(workspace.root.join("packages/lib")).expect("create the project root");
    std::os::unix::fs::symlink(&vendor, workspace.root.join("packages/lib/dist"))
        .expect("create the symlink");
    let mut workspace = workspace;
    let project_root = workspace.root.join("packages/lib");
    workspace.projects_by_path
        .get_mut(&ProjectPathKey::new(&project_root))
        .expect("the lib project")
        .linked_publish_dir = Some(project_root.join("dist"));

    assert_eq!(workspace.approve_package_at("packages/lib/dist"), approvals(&[]));
}

#[test]
fn approves_the_publish_directory_of_a_legacy_workspace_project() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/dist");
    let workspace_projects = [NamedProjectRoot {
        name: "lib".to_string(),
        root_dir: workspace.root.join("packages/lib"),
        linked_publish_dir: Some(workspace.root.join("packages/lib/dist")),
    }];
    let mut allow_builds = approvals(&[]);

    approve_legacy_workspace_project_builds(
        &mut allow_builds,
        &workspace_projects,
        &workspace.root,
        &workspace.root,
    )
    .expect("approve legacy workspace project builds");

    assert_eq!(
        allow_builds,
        approvals(&[("lib@file:packages/lib", true), ("lib@file:packages/lib/dist", true)]),
    );
}

#[test]
fn keeps_a_source_workspace_denial_of_the_publish_directory_of_a_legacy_workspace_project() {
    let workspace = Workspace::with_linked_publish_dir("packages/lib/dist");
    let workspace_projects = [NamedProjectRoot {
        name: "lib".to_string(),
        root_dir: workspace.root.join("packages/lib"),
        linked_publish_dir: Some(workspace.root.join("packages/lib/dist")),
    }];
    let mut allow_builds = approvals(&[("lib@file:packages/lib/dist", false)]);

    approve_legacy_workspace_project_builds(
        &mut allow_builds,
        &workspace_projects,
        &workspace.root,
        &workspace.root,
    )
    .expect("approve legacy workspace project builds");

    assert_eq!(allow_builds, approvals(&[("lib@file:packages/lib/dist", false)]));
}
