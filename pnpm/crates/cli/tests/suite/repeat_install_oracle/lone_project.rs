//! The gate checks of [`super`] for a repository whose only project is its
//! root. No project can depend on that root, so edits that matter in a
//! workspace, such as a version bump, change nothing here.

use super::{
    CURRENT_DRIVER, Driver, Layout, ProjectLockfileEntry, TreeView, WorkspaceFixture, copy_sources,
    driver_command_at, lockfile_entries, pnpm_at, run_across_drivers, took_install_fast_path,
    workspace_bins, workspace_lockfiles, workspace_modules_manifests,
};

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Output,
};

use serde_json::{Value, json};

/// What an install leaves behind that a user can observe.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    tree: String,
    bins: BTreeMap<String, Vec<String>>,
    lockfiles: BTreeMap<String, String>,
    modules: BTreeMap<String, pnpm_modules_yaml::Modules>,
}

/// Where the dependencies resolve from, the commands `node_modules/.bin`
/// offers, and what the wanted and the current lockfile record.
type RunView = (String, BTreeMap<String, Vec<String>>, [Option<ProjectLockfileEntry>; 2]);

struct LoneProject {
    driver: Driver,
    fixture: WorkspaceFixture,
    oracle_runs: usize,
}

impl LoneProject {
    fn new(layout: Layout) -> Self {
        let driver = CURRENT_DRIVER.with(|driver| driver.borrow().clone());
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(&layout.workspace_yaml());
        let project = Self { driver, fixture, oracle_runs: 0 };
        project.write_manifest(json!({}));
        project
    }

    fn root(&self) -> &Path {
        &self.fixture.workspace
    }

    /// The project's manifest with `fields` set over its defaults.
    fn write_manifest(&self, fields: Value) {
        let mut manifest = json!({
            "name": "lone",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "100.0.0" },
        });
        let Value::Object(fields) = fields else { panic!("the fields must be an object") };
        manifest
            .as_object_mut()
            .expect("a manifest object")
            .extend(fields);
        fs::write(
            self.root().join("package.json"),
            serde_json::to_string_pretty(&manifest).expect("serialize"),
        )
        .expect("write package.json");
    }

    fn command<Args, Arg>(&self, args: Args) -> Output
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
    {
        driver_command_at(&self.driver, &self.fixture, self.root(), args)
    }

    fn install(&self, args: &[&str]) -> bool {
        took_install_fast_path(&self.command(
            ["install", "--no-frozen-lockfile"].iter().chain(args),
        ))
    }

    /// Whether `exec` runs under `verifyDepsBeforeRun: error`.
    fn exec_passes(&self) -> bool {
        self.command(["--config.verify-deps-before-run=error", "exec", "node", "-e", ""])
            .status
            .success()
    }

    /// A clean install of a copy of the sources.
    fn oracle_root(&mut self) -> PathBuf {
        self.oracle_runs += 1;
        let copy = self
            .root()
            .with_file_name(format!("oracle-{}", self.oracle_runs));
        copy_sources(self.root(), &copy);
        let output = pnpm_at(&self.fixture, None, &copy, ["install", "--no-frozen-lockfile"]);
        super::assert_success(&output);
        copy
    }

    /// Both gates let the project through exactly when a clean install
    /// changes nothing, and do so right after an install.
    #[track_caller]
    fn check_gates(&mut self) {
        let oracle = self.oracle_root();
        let passed = self.exec_passes();
        let actual = run_view(self.root());
        let expected = run_view(&oracle);
        assert_eq!(
            passed,
            actual == expected,
            "{:?}: the run gate passed={passed}, but a clean install gives\n  actual: \
             {actual:#?}\n  expected: {expected:#?}",
            self.driver,
        );
        let before = snapshot(self.root());
        if self.install(&[]) {
            assert_eq!(
                snapshot(self.root()),
                before,
                "{:?}: the fast path modified the snapshot",
                self.driver,
            );
            assert!(!self.install(&["--config.optimistic-repeat-install=false"]));
            assert_eq!(
                snapshot(self.root()),
                before,
                "{:?}: the install fast path was taken, but a full install changes the tree",
                self.driver,
            );
        } else {
            assert_ne!(
                snapshot(self.root()),
                before,
                "{:?}: the install fast path was refused, but the install changed nothing",
                self.driver,
            );
        }
        assert!(self.install(&[]), "{:?}: a repeat install must take the fast path", self.driver);
        assert!(
            self.exec_passes(),
            "{:?}: the run gate must pass right after an install",
            self.driver,
        );
    }
}

fn snapshot(root: &Path) -> Snapshot {
    Snapshot {
        tree: TreeView::new(root).project(root, true),
        bins: workspace_bins(root),
        lockfiles: workspace_lockfiles(root),
        modules: workspace_modules_manifests(root),
    }
}

fn run_view(root: &Path) -> RunView {
    (TreeView::new(root).project(root, true), workspace_bins(root), lockfile_entries(root, "."))
}

/// Edits that change no dependency, each kept by the next, then one that
/// does.
fn manifest_edits(layout: Layout) {
    let mut project = LoneProject::new(layout);
    project.install(&[]);
    let mut fields = json!({});
    for (field, value) in [
        ("description", json!("edited")),
        ("scripts", json!({ "build": r#"node -e """# })),
        ("version", json!("1.0.1")),
        ("version", json!("2.0.0")),
        (
            "dependencies",
            json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }),
        ),
    ] {
        fields[field] = value;
        project.write_manifest(fields.clone());
        project.check_gates();
    }
}

#[test]
fn isolated() {
    run_across_drivers(
        Layout { hoisted: false, shared_lockfile: true, inject_workspace_packages: false },
        manifest_edits,
    );
}

#[test]
fn hoisted() {
    run_across_drivers(
        Layout { hoisted: true, shared_lockfile: true, inject_workspace_packages: false },
        manifest_edits,
    );
}
