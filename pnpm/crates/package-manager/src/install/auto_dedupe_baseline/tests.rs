use super::{AutoDedupeBaseline, BaselineInputs, RECORD_DIR};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::WorkspaceStateSettings;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

const LOCKFILE: &str = "lockfileVersion: '9.0'

packages:

  local@file:local:
    resolution: {directory: local, type: directory}

  packed@file:packed.tgz:
    resolution: {tarball: file:packed.tgz}
";

struct Workspace {
    root: TempDir,
    cache: TempDir,
    settings: WorkspaceStateSettings,
    ignore_pnpmfile: bool,
    manifests: Vec<(PathBuf, PackageManifest)>,
}

impl Workspace {
    fn new() -> Self {
        let root = TempDir::new().expect("workspace dir");
        let cache = TempDir::new().expect("cache dir");
        fs::write(root.path().join("pnpm-lock.yaml"), LOCKFILE).expect("write lockfile");
        fs::create_dir_all(root.path().join("local")).expect("create local package");
        fs::write(root.path().join("local/package.json"), r#"{"name":"local"}"#)
            .expect("write local manifest");
        fs::write(root.path().join("packed.tgz"), "packed").expect("write local tarball");
        let manifests = [(".", "root"), ("packages/a", "a")]
            .into_iter()
            .map(|(dir, name)| {
                let dir = root.path().join(dir);
                let value = serde_json::json!({"name": name, "version": "1.0.0"});
                (dir.clone(), PackageManifest::from_value(dir.join("package.json"), value))
            })
            .collect();
        let settings =
            WorkspaceStateSettings { auto_dedupe: Some(true), ..WorkspaceStateSettings::default() };
        Workspace { root, cache, settings, ignore_pnpmfile: false, manifests }
    }

    fn lockfile_path(&self) -> PathBuf {
        self.root.path().join("pnpm-lock.yaml")
    }

    fn lockfile(&self) -> Lockfile {
        let content = fs::read_to_string(self.lockfile_path()).expect("read lockfile");
        Lockfile::parse(&content, &self.lockfile_path())
            .expect("lockfile parses")
            .expect("lockfile is non-empty")
    }

    fn baseline(&self) -> AutoDedupeBaseline {
        let project_manifests: Vec<(PathBuf, &PackageManifest)> = self.manifests
            .iter()
            .map(|(dir, manifest)| (dir.clone(), manifest))
            .collect();
        AutoDedupeBaseline::new(
            self.cache.path(),
            self.lockfile_path(),
            &BaselineInputs {
                settings: &self.settings,
                ignore_pnpmfile: self.ignore_pnpmfile,
                workspace_root: self.root.path(),
                project_manifests: &project_manifests,
            },
        )
    }

    fn record(&self) {
        self.baseline().record(&self.lockfile());
    }

    fn matches(&self) -> bool {
        self.baseline()
            .matches(&self.lockfile())
    }
}

#[test]
fn a_rewritten_lockfile_misses() {
    let workspace = Workspace::new();
    workspace.record();
    fs::write(workspace.lockfile_path(), format!("{LOCKFILE}\n")).expect("rewrite lockfile");
    assert!(!workspace.matches());
}

#[test]
fn a_local_directory_without_a_manifest_is_not_recorded() {
    let workspace = Workspace::new();
    fs::remove_file(workspace.root.path().join("local/package.json"))
        .expect("remove local manifest");
    workspace.record();
    assert!(
        !workspace.cache
            .path()
            .join(RECORD_DIR)
            .exists(),
    );
}

#[test]
fn a_changed_local_tarball_misses() {
    let workspace = Workspace::new();
    workspace.record();
    fs::write(workspace.root.path().join("packed.tgz"), "repacked").expect("repack tarball");
    assert!(!workspace.matches());
}

#[test]
fn ignoring_the_pnpmfile_misses() {
    let mut workspace = Workspace::new();
    workspace.record();
    workspace.ignore_pnpmfile = true;
    assert!(!workspace.matches());
}

#[test]
fn project_order_does_not_change_the_record() {
    let mut workspace = Workspace::new();
    workspace.record();
    workspace.manifests.reverse();
    assert!(workspace.matches());
}

#[test]
fn an_unchanged_workspace_matches() {
    let workspace = Workspace::new();
    workspace.record();
    assert!(workspace.matches());
}

#[test]
fn a_changed_project_manifest_misses() {
    let mut workspace = Workspace::new();
    workspace.record();
    let (dir, _) = &workspace.manifests[1];
    let value = serde_json::json!({"name": "a", "version": "1.0.1"});
    workspace.manifests[1].1 = PackageManifest::from_value(dir.join("package.json"), value);
    assert!(!workspace.matches());
}
