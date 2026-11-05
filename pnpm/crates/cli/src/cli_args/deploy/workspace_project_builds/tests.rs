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
        NamedProjectRoot { name: "lib".to_string(), root_dir: workspace.root.join("packages/lib") },
        NamedProjectRoot {
            name: "denied".to_string(),
            root_dir: workspace.root.join("packages/denied"),
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
