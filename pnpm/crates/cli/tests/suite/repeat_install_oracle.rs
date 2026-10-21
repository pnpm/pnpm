//! Sequences of installs, filtered installs, and edits, checked against the
//! install a gate stands in for, across lockfile layouts, linkers, and
//! injected workspace packages.
//!
//! After every step, each gate is checked in both directions:
//!
//! - **Install gate.** `pnpm install` takes its fast path exactly when a full
//!   install (`optimisticRepeatInstall: false`) of the same tree changes
//!   nothing in `node_modules` or the lockfiles.
//! - **Run gate.** `verifyDepsBeforeRun: error` lets a filtered command run
//!   exactly when a clean install of a copy of the sources resolves the
//!   selected projects' dependencies the same way and records the same
//!   lockfile entries for them.
//!
//! Right after an install, every gate it satisfied reports the tree up to
//! date without installing again.

pub use _utils::*;

use crate::_utils;

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime},
};

use pnpm_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use pnpm_testing_utils::command_env::CommandTestExt;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
enum Driver {
    V12,
    V11(OsString),
    Mixed { bundle: OsString, next_v11: Cell<bool> },
}

thread_local! {
    static CURRENT_DRIVER: RefCell<Driver> = const { RefCell::new(Driver::V12) };
}

fn run_across_drivers(layout: Layout, scenario: fn(Layout)) {
    CURRENT_DRIVER.with(|driver| *driver.borrow_mut() = Driver::V12);
    scenario(layout);
    if let Some(bundle) = read_v11_bundle_path() {
        CURRENT_DRIVER.with(|driver| *driver.borrow_mut() = Driver::V11(bundle.clone()));
        scenario(layout);
        if layout.shared_lockfile {
            CURRENT_DRIVER.with(|driver| {
                *driver.borrow_mut() = Driver::Mixed { bundle, next_v11: Cell::new(false) }
            });
            scenario(layout);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    hoisted: bool,
    shared_lockfile: bool,
    inject_workspace_packages: bool,
}

impl Layout {
    fn workspace_yaml(self) -> String {
        format!(
            "nodeLinker: {}\nsharedWorkspaceLockfile: {}\ninjectWorkspacePackages: {}\n",
            if self.hoisted { "hoisted" } else { "isolated" },
            self.shared_lockfile,
            self.inject_workspace_packages,
        )
    }
}

const PROJECTS: [&str; 3] = ["a", "b", "c"];

struct Scenario {
    driver: Driver,
    fixture: WorkspaceFixture,
    layout: Layout,
    oracle_runs: usize,
}

/// What an install leaves behind that a user can observe: where every
/// dependency resolves from each project, every lockfile, and every
/// `.modules.yaml` manifest.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    tree: BTreeMap<String, String>,
    lockfiles: BTreeMap<String, String>,
    modules: BTreeMap<String, pnpm_modules_yaml::Modules>,
}

/// What a lockfile records for one project: its importer entry, and for a
/// dedicated lockfile its package snapshots (including injected workspace
/// packages).
type ProjectLockfileEntry = (ProjectSnapshot, Option<HashMap<PackageKey, SnapshotEntry>>);

/// Where the selected projects' dependencies resolve from, and the entries
/// that record them in the wanted and the current lockfiles.
type SelectionView =
    (BTreeMap<String, String>, BTreeMap<String, [Option<ProjectLockfileEntry>; 2]>);

impl Scenario {
    /// A workspace whose `c` depends on its sibling `a`, so a selection reaches
    /// a project it did not name, and whose `b` has a dev dependency.
    fn new(layout: Layout) -> Self {
        let driver = CURRENT_DRIVER.with(|driver| driver.borrow().clone());
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(&layout.workspace_yaml());
        fixture.write_root_manifest("root", ManifestDeps::default());
        let scenario = Self { driver, fixture, layout, oracle_runs: 0 };
        scenario.set_dependencies("a", &json!({ "@pnpm.e2e/foo": "100.0.0" }));
        scenario.write_manifest(
            "b",
            json!({
                "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
                "devDependencies": { "@pnpm.e2e/bar": "100.0.0" },
            }),
        );
        scenario.set_dependencies("c", &json!({ "a": "workspace:*", "@pnpm.e2e/bar": "100.0.0" }));
        scenario
    }

    fn command_at_with<Args, Arg>(
        &self,
        v11_bundle: Option<&OsStr>,
        cwd: &Path,
        args: Args,
    ) -> Output
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
    {
        if let Some(bundle) = v11_bundle {
            return self.fixture.v11_command_at(bundle, cwd, args);
        }
        pacquet_in(cwd)
            .without_ambient_pnpm_config()
            .current_dir(cwd)
            .env("PNPM_CONFIG_REGISTRY", self.fixture.registry.mock_instance.url())
            .env("XDG_CONFIG_HOME", self.fixture.workspace.with_file_name("xdg-config"))
            .arg("--reporter=ndjson")
            .args(args)
            .output()
            .expect("run pnpm")
    }

    fn command_at<Args, Arg>(&self, cwd: &Path, args: Args) -> Output
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
    {
        match &self.driver {
            Driver::V12 => self.command_at_with(None, cwd, args),
            Driver::V11(bundle) => self.command_at_with(Some(bundle), cwd, args),
            Driver::Mixed { bundle, next_v11 } => {
                let use_v11 = next_v11.get();
                next_v11.set(!use_v11);
                self.command_at_with(use_v11.then_some(bundle.as_os_str()), cwd, args)
            }
        }
    }

    fn workspace(&self) -> &Path {
        &self.fixture.workspace
    }

    fn manifest_path(&self, project: &str) -> PathBuf {
        self.workspace()
            .join("packages")
            .join(project)
            .join("package.json")
    }

    /// The lockfile that records `project`.
    fn lockfile_of(&self, project: &str) -> PathBuf {
        if self.layout.shared_lockfile {
            self.workspace().join("pnpm-lock.yaml")
        } else {
            self.workspace()
                .join("packages")
                .join(project)
                .join("pnpm-lock.yaml")
        }
    }

    fn set_dependencies(&self, project: &str, dependencies: &Value) {
        self.write_manifest(project, json!({ "dependencies": dependencies }));
    }

    fn write_manifest(&self, project: &str, groups: Value) {
        let path = self.manifest_path(project);
        fs::create_dir_all(path.parent().expect("project dir")).expect("create project dir");
        let mut manifest = json!({ "name": project, "version": "1.0.0", "private": true });
        let Value::Object(groups) = groups else { panic!("the groups must be an object") };
        manifest
            .as_object_mut()
            .expect("a manifest object")
            .extend(groups);
        fs::write(&path, serde_json::to_string_pretty(&manifest).expect("serialize"))
            .expect("write package.json");
    }

    /// Whether the CLI that runs the next `pnpm install` offers its fast path
    /// in this layout.
    ///
    /// A dedicated lockfile records an injected package's dependencies in its
    /// `file:` snapshot, which the install fast path does not compare, so both
    /// CLIs refuse it (pnpm/tasks#145). With a shared lockfile, v11 also
    /// refuses the install fast path to refresh injected copies (pnpm/pnpm#15679).
    fn offers_next_install_fast_path(&self) -> bool {
        if !self.layout.inject_workspace_packages {
            return true;
        }
        let next_is_v11 = match &self.driver {
            Driver::V12 => false,
            Driver::V11(_) => true,
            Driver::Mixed { next_v11, .. } => next_v11.get(),
        };
        self.layout.shared_lockfile && !next_is_v11
    }

    /// Runs `pnpm install` with `args` and tells whether it took the fast
    /// path, or ran in a layout that never offers it.
    fn install_takes_fast_path(&self, args: &[&str]) -> bool {
        let offered = self.offers_next_install_fast_path();
        self.install(args) || !offered
    }

    /// `pnpm install` with `args`, returning whether it took the fast path:
    /// it said "Already up to date" without starting a resolution or an
    /// import.
    fn install(&self, args: &[&str]) -> bool {
        let output = self.command_at(
            self.workspace(),
            ["install", "--no-frozen-lockfile"].iter().chain(args),
        );
        assert_success(&output);
        let records = ndjson_records(&output);
        let up_to_date = records
            .iter()
            .any(|record| record["message"] == "Already up to date");
        let staged = records
            .iter()
            .any(|record| record["name"] == "pnpm:stage");
        up_to_date && !staged
    }

    /// Run `node -e ""` through `exec` for `selection` under
    /// `verifyDepsBeforeRun: action`, returning whether the command ran.
    fn exec(&self, selection: &[&str], action: &str) -> bool {
        let args = exec_args(selection, action);
        let check_output = |output: Output| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("left node_modules out of date"),
                "{:?} ({:?}): the install the gate spawned for {selection:?} did not converge:\n{stderr}",
                self.layout,
                self.driver,
            );
            output.status.success()
        };
        if action == "error"
            && let Driver::Mixed { bundle, next_v11 } = &self.driver
        {
            let first_v11 = next_v11.get();
            next_v11.set(!first_v11);
            let first_out = self.command_at_with(
                first_v11.then_some(bundle.as_os_str()),
                self.workspace(),
                &args,
            );
            let first_err = String::from_utf8_lossy(&first_out.stderr).into_owned();
            let first_std = String::from_utf8_lossy(&first_out.stdout).into_owned();
            let first = check_output(first_out);
            let second_out = self.command_at_with(
                (!first_v11).then_some(bundle.as_os_str()),
                self.workspace(),
                &args,
            );
            let second_err = String::from_utf8_lossy(&second_out.stderr).into_owned();
            let second_std = String::from_utf8_lossy(&second_out.stdout).into_owned();
            let second = check_output(second_out);
            assert_eq!(
                first, second,
                "{:?}: v11 and v12 run gates disagreed for {selection:?} (first_v11={first_v11})\nfirst stdout: {first_std}\nfirst stderr: {first_err}\nsecond stdout: {second_std}\nsecond stderr: {second_err}",
                self.layout,
            );
            return first;
        }
        check_output(self.command_at(self.workspace(), &args))
    }

    /// [`Self::exec`] without a filter, from `project` (`.` for the root).
    fn exec_from(&self, project: &str, action: &str) -> bool {
        let cwd = self.workspace().join(project);
        let args =
            [&*format!("--config.verify-deps-before-run={action}"), "exec", "node", "-e", ""];
        if action == "error"
            && let Driver::Mixed { bundle, next_v11 } = &self.driver
        {
            let first_v11 = next_v11.get();
            next_v11.set(!first_v11);
            let first = self
                .command_at_with(first_v11.then_some(bundle.as_os_str()), &cwd, args)
                .status
                .success();
            let second = self
                .command_at_with((!first_v11).then_some(bundle.as_os_str()), &cwd, args)
                .status
                .success();
            assert_eq!(
                first, second,
                "{:?}: v11 and v12 run gates disagreed from {project} (first_v11={first_v11})",
                self.layout,
            );
            return first;
        }
        self.command_at(&cwd, args).status.success()
    }

    /// Drop the validation records from every state.
    fn strip_validation(&self) {
        for state_path in walk_files(self.workspace())
            .into_iter()
            .filter(|path| path.ends_with("node_modules/.pnpm-workspace-state-v1.json"))
        {
            let mut state: Value =
                serde_json::from_str(&fs::read_to_string(&state_path).expect("read the state"))
                    .expect("parse the state");
            state
                .as_object_mut()
                .expect("a state object")
                .remove("validation");
            fs::write(&state_path, serde_json::to_string_pretty(&state).expect("serialize"))
                .expect("write the state");
        }
    }

    /// Where every dependency of `projects` resolves from in the tree under
    /// `workspace`. Dev dependencies count only when `with_dev` is set.
    fn tree(workspace: &Path, projects: &[&str], with_dev: bool) -> BTreeMap<String, String> {
        let mut resolver = TreeView::new(workspace);
        projects
            .iter()
            .map(|project| {
                let dir = workspace.join("packages").join(project);
                ((*project).to_string(), resolver.project(&dir, with_dev))
            })
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tree: Self::tree(self.workspace(), &PROJECTS, true),
            lockfiles: workspace_lockfiles(self.workspace()),
            modules: workspace_modules_manifests(self.workspace()),
        }
    }

    /// The entries that record `projects` in the wanted and the current
    /// lockfiles.
    fn importers(
        &self,
        workspace: &Path,
        projects: &[&str],
    ) -> BTreeMap<String, [Option<ProjectLockfileEntry>; 2]> {
        let dedicated = !self.layout.shared_lockfile;
        let entry = |lockfile: PathBuf, importer_id: &str| {
            lockfile
                .is_file()
                .then(|| read_lockfile(&lockfile))
                .and_then(|lockfile| {
                    Some((
                        lockfile.importers.get(importer_id)?.clone(),
                        dedicated.then_some(lockfile.snapshots).flatten(),
                    ))
                })
        };
        projects
            .iter()
            .map(|project| {
                let (dir, importer_id) = if self.layout.shared_lockfile {
                    (workspace.to_path_buf(), format!("packages/{project}"))
                } else {
                    (workspace.join("packages").join(project), ".".into())
                };
                let wanted = entry(dir.join("pnpm-lock.yaml"), &importer_id);
                let current = entry(dir.join("node_modules/.pnpm/lock.yaml"), &importer_id);
                ((*project).to_string(), [wanted, current])
            })
            .collect()
    }

    fn selection_view(&self, workspace: &Path, selection: &[&str]) -> SelectionView {
        (Self::tree(workspace, selection, false), self.importers(workspace, selection))
    }

    /// Clean-installed copy of the sources.
    fn oracle_workspace(&mut self) -> PathBuf {
        self.oracle_runs += 1;
        let copy = self
            .workspace()
            .with_file_name(format!("oracle-{}", self.oracle_runs));
        copy_sources(self.workspace(), &copy);
        let mut args = vec!["install", "--no-frozen-lockfile"];
        if self.recorded_production_only() {
            args.push("--prod");
        }
        let output = self.command_at_with(None, &copy, args);
        assert_success(&output);
        copy
    }

    /// Whether the last install left the dev dependencies out, so a run gate
    /// holds the tree to a production install.
    fn recorded_production_only(&self) -> bool {
        let candidates = if self.layout.shared_lockfile {
            vec![self.workspace().to_path_buf()]
        } else {
            vec![self.workspace().join("packages/a"), self.workspace().to_path_buf()]
        };
        candidates
            .into_iter()
            .find_map(|state_dir| {
                fs::read_to_string(state_dir.join("node_modules/.pnpm-workspace-state-v1.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            })
            .is_some_and(|state| state["settings"]["dev"] == false)
    }

    /// The run gate for `selection` lets the command through exactly when
    /// the selection already is what a clean install makes it.
    #[track_caller]
    fn assert_run_gate(&self, oracle: &Path, selection: &[&str]) {
        let passed = self.exec(selection, "error");
        let actual = self.selection_view(self.workspace(), selection);
        let expected = self.selection_view(oracle, selection);
        let current = actual == expected;
        assert!(
            !passed || current,
            "{:?} ({:?}): the run gate for {selection:?} let the command run, but a clean install \
             differs:\n  actual: {actual:#?}\n  expected: {expected:#?}",
            self.layout,
            self.driver,
        );
        assert!(
            passed || !current,
            "{:?} ({:?}): the run gate for {selection:?} refused, but a clean install changes nothing \
             for it",
            self.layout,
            self.driver,
        );
    }

    /// `pnpm install` takes its fast path exactly when a full install changes
    /// nothing, and takes it on the next run. `conservative` allows a refusal
    /// for a tree a full install leaves as it is.
    #[track_caller]
    fn assert_install_gate(&self, conservative: bool) {
        let before = self.snapshot();
        let offered = self.offers_next_install_fast_path();
        if self.install(&[]) {
            assert_eq!(
                self.snapshot(),
                before,
                "{:?} ({:?}): the install fast path modified the snapshot",
                self.layout,
                self.driver,
            );
            assert!(!self.install(&["--config.optimistic-repeat-install=false"]));
            assert_eq!(
                self.snapshot(),
                before,
                "{:?} ({:?}): the install fast path was taken, but a full install changes the tree",
                self.layout,
                self.driver,
            );
        } else if !conservative && offered {
            assert_ne!(
                self.snapshot(),
                before,
                "{:?} ({:?}): the install fast path was refused, but the install changed nothing",
                self.layout,
                self.driver,
            );
        }
        let settled = self.snapshot();
        let raw_modules_before_repeat = workspace_raw_modules_manifests(self.workspace());
        let repeat_offered = self.offers_next_install_fast_path();
        assert!(
            self.install_takes_fast_path(&[]),
            "{:?} ({:?}): a repeat install must take the fast path",
            self.layout,
            self.driver,
        );
        assert_eq!(
            self.snapshot(),
            settled,
            "{:?} ({:?}): a repeat install must be byte-identical",
            self.layout,
            self.driver,
        );
        if repeat_offered {
            assert_eq!(
                workspace_raw_modules_manifests(self.workspace()),
                raw_modules_before_repeat,
                "{:?} ({:?}): a fast-path repeat install must not rewrite .modules.yaml",
                self.layout,
                self.driver,
            );
        }
        if matches!(self.driver, Driver::Mixed { .. }) {
            let raw_modules_before_cross = workspace_raw_modules_manifests(self.workspace());
            let cross_repeat_offered = self.offers_next_install_fast_path();
            assert!(
                self.install_takes_fast_path(&[]),
                "{:?} ({:?}): a cross-CLI repeat install must take the fast path",
                self.layout,
                self.driver,
            );
            assert_eq!(
                self.snapshot(),
                settled,
                "{:?} ({:?}): a cross-CLI repeat install must be byte-identical",
                self.layout,
                self.driver,
            );
            if cross_repeat_offered {
                assert_eq!(
                    workspace_raw_modules_manifests(self.workspace()),
                    raw_modules_before_cross,
                    "{:?} ({:?}): a cross-CLI fast-path repeat install must not rewrite .modules.yaml",
                    self.layout,
                    self.driver,
                );
            }
        }
        let frozen = self.command_at(self.workspace(), ["install", "--frozen-lockfile"]);
        assert_success(&frozen);
        assert_eq!(
            self.snapshot(),
            settled,
            "{:?} ({:?}): a frozen install must accept and preserve the converged snapshot",
            self.layout,
            self.driver,
        );
        if self.layout.shared_lockfile {
            let dedupe = self.command_at(self.workspace(), ["dedupe", "--check"]);
            assert_success(&dedupe);
        }
    }

    /// Every gate over the workspace, then the convergence the install leaves.
    #[track_caller]
    fn check_gates(&mut self) {
        self.check_gates_allowing(false);
    }

    #[track_caller]
    fn check_gates_allowing(&mut self, conservative: bool) {
        let oracle = self.oracle_workspace();
        for selection in [&["a"][..], &["b"], &["c"], &PROJECTS] {
            self.assert_run_gate(&oracle, selection);
        }
        self.assert_install_gate(conservative);
        for selection in [&["a"][..], &["b"], &["c"], &PROJECTS] {
            assert!(
                self.exec(selection, "error"),
                "{:?} ({:?}): the run gate for {selection:?} must pass right after a full install",
                self.layout,
                self.driver,
            );
        }
    }

    /// `exec` under the `install` action, then the properties that action
    /// promises: the command ran, and the gate is quiet on the next run.
    #[track_caller]
    fn exec_installing(&self, selection: &[&str]) {
        assert!(self.exec(selection, "install"), "the gated command must run");
        assert!(
            self.exec(selection, "error"),
            "{:?}: the install the gate spawned for {selection:?} must satisfy the gate",
            self.layout,
        );
        let repeat_count = if matches!(&self.driver, Driver::Mixed { .. }) { 2 } else { 1 };
        for _ in 0..repeat_count {
            let repeat = self.command_at(self.workspace(), exec_args(selection, "install"));
            assert_success(&repeat);
            let records = ndjson_records(&repeat);
            assert!(
                !records
                    .iter()
                    .any(|record| record["name"] == "pnpm:stage"),
                "{:?} ({:?}): the run gate for {selection:?} must be quiet after the install it spawned",
                self.layout,
                self.driver,
            );
        }
    }
}

fn exec_args(selection: &[&str], action: &str) -> Vec<String> {
    let mut args = vec![format!("--config.verify-deps-before-run={action}")];
    for project in selection {
        args.push("--filter".to_string());
        args.push((*project).to_string());
    }
    args.extend(["exec", "node", "-e", ""].map(str::to_string));
    args
}

/// Overwrite `path` with `contents`, keeping the mtime it had: what `cp -p`,
/// `rsync -a`, an archive extraction, or a Docker `COPY` of an older file
/// leaves behind.
fn write_keeping_mtime(path: &Path, contents: &str) {
    let mtime =
        fs::metadata(path).and_then(|metadata| metadata.modified()).expect("read the mtime");
    fs::write(path, contents).expect("write the file");
    set_mtime(path, mtime);
}

fn rewrite_unchanged(path: &Path) {
    let contents = fs::read(path).expect("read the file");
    fs::write(path, contents).expect("rewrite the file");
}

fn set_mtime(path: &Path, mtime: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(mtime))
        .expect("set the mtime");
}

fn a_full_install_converges(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    assert!(!scenario.install(&[]));
    scenario.check_gates();
}

/// pnpm/pnpm#17011: a pull changes two projects, and a filtered command
/// installs only one of them.
fn a_pull_then_a_filtered_command_for_one_project(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    for project in ["a", "b"] {
        scenario.set_dependencies(
            project,
            &json!({ "@pnpm.e2e/foo": "100.0.0", "@pnpm.e2e/bar": "100.0.0" }),
        );
    }
    scenario.install(&["--lockfile-only"]);
    scenario.exec_installing(&["a"]);
    scenario.check_gates();
}

fn a_filtered_install_into_a_fresh_workspace(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&["--filter", "b"]);
    scenario.check_gates();
}

/// A filtered install for `c` installs its workspace dependency `a` too.
fn a_filtered_install_reaching_a_workspace_dependency(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&["--filter", "c"]);
    assert!(
        scenario.install_takes_fast_path(&["--filter", "c"]),
        "{layout:?}: a repeat of the same filtered install must take the fast path",
    );
    scenario.check_gates();
}

/// An edit to a project the next filtered install does not select.
fn an_edit_outside_the_next_filtered_install(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
    scenario.install(&["--filter", "a"]);
    scenario.check_gates();
}

fn filtered_commands_for_every_project_after_a_pull(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    scenario.set_dependencies("a", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/foo": "100.0.0" }));
    scenario.install(&["--lockfile-only"]);
    for project in PROJECTS {
        scenario.exec_installing(&[project]);
    }
    assert!(
        scenario.install_takes_fast_path(&[]),
        "{layout:?}: once every project was installed, a full install has nothing to do",
    );
    scenario.check_gates();
}

/// A rewrite that keeps the content, as `touch`, a `git checkout` of the same
/// blob, or a formatter that changed nothing leaves.
fn rewritten_but_unchanged_inputs_keep_the_fast_path(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    rewrite_unchanged(&scenario.lockfile_of("a"));
    rewrite_unchanged(&scenario.manifest_path("a"));
    assert!(
        scenario.install_takes_fast_path(&[]),
        "{layout:?}: unchanged inputs keep the fast path",
    );
    scenario.check_gates();
}

/// An edit that leaves the manifest's mtime where it was.
fn a_manifest_edit_that_keeps_its_mtime(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    let path = scenario.manifest_path("b");
    let edited = fs::read_to_string(&path)
        .expect("read package.json")
        .replace("@pnpm.e2e/pkg-with-1-dep", "@pnpm.e2e/foo");
    write_keeping_mtime(&path, &edited);
    scenario.check_gates();
}

/// A manifest and a lockfile swapped for ones whose mtimes predate the last
/// install, as restoring both from an older checkout leaves them.
fn a_lockfile_swap_that_keeps_an_older_mtime(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    let lockfile = scenario.lockfile_of("b");
    let manifest = scenario.manifest_path("b");
    let original = fs::read_to_string(&manifest).expect("read the manifest");
    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/foo": "100.0.0" }));
    let edited = fs::read_to_string(&manifest).expect("read the manifest");
    scenario.install(&["--lockfile-only"]);
    let swapped = fs::read_to_string(&lockfile).expect("read the lockfile");
    fs::write(&manifest, &original).expect("restore the manifest");
    scenario.install(&[]);
    fs::write(&lockfile, &swapped).expect("swap the lockfile");
    fs::write(&manifest, &edited).expect("swap the manifest");
    let an_hour_ago = SystemTime::now() - Duration::from_hours(1);
    set_mtime(&lockfile, an_hour_ago);
    set_mtime(&manifest, an_hour_ago);
    scenario.check_gates();
}

/// The install an unfiltered command spawns covers the whole workspace, so a
/// project another filtered install left out is installed only where the
/// command needs it.
fn an_unfiltered_command_after_a_filtered_install_of_another_project(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&["--filter", "b"]);
    assert!(
        scenario.exec_from("packages/b", "error"),
        "{layout:?}: a command in the installed project does not need the projects the install \
         left out",
    );
    assert!(
        !scenario.exec_from("packages/a", "error"),
        "{layout:?}: a command in a project the filtered install left out needs it installed",
    );
    assert!(scenario.exec_from("packages/a", "install"), "the gated command must run");
    assert!(scenario.exec_from("packages/a", "error"), "the gate's install must satisfy it");
    scenario.check_gates();
}

/// A root script may act on every project, so an unfiltered command from the
/// root is held to an edit anywhere in a workspace with one lockfile.
fn a_root_command_after_an_edit_in_a_project(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
    if layout.shared_lockfile {
        assert!(!scenario.exec_from(".", "error"), "{layout:?}: the edited project is stale");
        assert!(scenario.exec_from(".", "install"), "the gated command must run");
        assert!(scenario.exec_from(".", "error"), "the gate's install must satisfy it");
    }
    scenario.check_gates();
}

/// A state without validation records is proven again from the lockfiles once
/// and then recorded. After a filtered install the install gate refuses it
/// outright: the current lockfile cannot show which projects that install
/// left out.
fn a_state_without_validation_records(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&[]);
    scenario.strip_validation();
    scenario.check_gates();

    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
    scenario.install(&["--filter", "a"]);
    scenario.strip_validation();
    scenario.check_gates_allowing(true);
}

/// A production install leaves the dev dependencies out, and an install of
/// every group after it adds them.
fn a_production_install_then_every_group(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    scenario.install(&["--prod"]);
    scenario.check_gates();
}

/// A workspace root without a manifest is no importer.
fn a_workspace_without_a_root_manifest(layout: Layout) {
    let mut scenario = Scenario::new(layout);
    fs::remove_file(scenario.workspace().join("package.json")).expect("remove the root manifest");
    scenario.install(&["--filter", "a"]);
    scenario.check_gates();
    scenario.set_dependencies("b", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
    scenario.check_gates();
}

macro_rules! matrix {
    ($($module:ident: $layout:expr,)*) => {
        $(
            mod $module {
                use super::Layout;

                const LAYOUT: Layout = $layout;

                matrix!(@tests
                    a_full_install_converges,
                    a_pull_then_a_filtered_command_for_one_project,
                    a_filtered_install_into_a_fresh_workspace,
                    a_filtered_install_reaching_a_workspace_dependency,
                    an_edit_outside_the_next_filtered_install,
                    filtered_commands_for_every_project_after_a_pull,
                    rewritten_but_unchanged_inputs_keep_the_fast_path,
                    a_manifest_edit_that_keeps_its_mtime,
                    a_lockfile_swap_that_keeps_an_older_mtime,
                    an_unfiltered_command_after_a_filtered_install_of_another_project,
                    a_root_command_after_an_edit_in_a_project,
                    a_state_without_validation_records,
                    a_production_install_then_every_group,
                    a_workspace_without_a_root_manifest,
                );
            }
        )*
    };
    (@tests $($scenario:ident,)*) => {
        $(
            #[test]
            fn $scenario() {
                super::run_across_drivers(LAYOUT, super::$scenario);
            }
        )*
    };
}

matrix! {
    isolated: Layout {
        hoisted: false,
        shared_lockfile: true,
        inject_workspace_packages: false,
    },
    isolated_injected: Layout {
        hoisted: false,
        shared_lockfile: true,
        inject_workspace_packages: true,
    },
    isolated_dedicated_lockfiles: Layout {
        hoisted: false,
        shared_lockfile: false,
        inject_workspace_packages: false,
    },
    isolated_dedicated_lockfiles_injected: Layout {
        hoisted: false,
        shared_lockfile: false,
        inject_workspace_packages: true,
    },
    hoisted: Layout {
        hoisted: true,
        shared_lockfile: true,
        inject_workspace_packages: false,
    },
    hoisted_injected: Layout {
        hoisted: true,
        shared_lockfile: true,
        inject_workspace_packages: true,
    },
    hoisted_dedicated_lockfiles: Layout {
        hoisted: true,
        shared_lockfile: false,
        inject_workspace_packages: false,
    },
    hoisted_dedicated_lockfiles_injected: Layout {
        hoisted: true,
        shared_lockfile: false,
        inject_workspace_packages: true,
    },
}

#[test]
fn ci_and_auto_dedupe_gate_converges_across_v11_and_v12() {
    let layout = Layout { hoisted: false, shared_lockfile: true, inject_workspace_packages: false };
    CURRENT_DRIVER.with(|driver| *driver.borrow_mut() = Driver::V12);
    let scenario = Scenario::new(layout);
    scenario.fixture.append_workspace_yaml("autoDedupe: true\n");

    let run_in_ci = |v11_bundle: Option<&OsStr>, args: &[&str]| {
        let command = if let Some(bundle) = v11_bundle {
            let mut cmd = Command::new("node");
            cmd.arg(bundle).arg("--config.confirm-modules-purge=false");
            cmd
        } else {
            pacquet_in(scenario.workspace())
        };
        command
            .without_ambient_pnpm_config()
            .current_dir(scenario.workspace())
            .env("CI", "true")
            .env("PNPM_CONFIG_CI", "true")
            .env("PNPM_CONFIG_REGISTRY", scenario.fixture.registry.mock_instance.url())
            .env("XDG_CONFIG_HOME", scenario.fixture.workspace.with_file_name("xdg-config"))
            .arg("--reporter=ndjson")
            .args(args)
            .output()
            .expect("run pnpm in CI")
    };

    let first_ci = run_in_ci(None, &["install", "--no-frozen-lockfile"]);
    assert_success(&first_ci);
    let second_ci = run_in_ci(None, &["install", "--no-frozen-lockfile"]);
    assert_success(&second_ci);
    let records = ndjson_records(&second_ci);
    assert!(
        records
            .iter()
            .any(|record| record["message"] == "Already up to date")
            && !records
                .iter()
                .any(|record| record["name"] == "pnpm:stage"),
        "v12 repeat install in CI with autoDedupe: true must take the fast path",
    );
    let ci_exec =
        run_in_ci(None, &["--config.verify-deps-before-run=error", "exec", "node", "-e", ""]);
    assert_success(&ci_exec);
    if let Some(bundle) = read_v11_bundle_path() {
        let v11_ci_repeat = run_in_ci(Some(&bundle), &["install", "--no-frozen-lockfile"]);
        assert_success(&v11_ci_repeat);
        let v11_ci_records = ndjson_records(&v11_ci_repeat);
        assert!(
            v11_ci_records
                .iter()
                .any(|record| record["message"] == "Already up to date")
                && !v11_ci_records
                    .iter()
                    .any(|record| record["name"] == "pnpm:stage"),
            "v11 repeat install in CI after v12 CI install must take the fast path",
        );
        let v11_ci_exec = run_in_ci(
            Some(&bundle),
            &["--config.verify-deps-before-run=error", "exec", "node", "-e", ""],
        );
        assert_success(&v11_ci_exec);
    }

    // Outside CI, v12 records `autoDedupe: true` in `pnpm-state.yaml`; v11 must
    // accept that state on its fast path and preserve `autoDedupe: true` when it
    // rewrites the state file so a subsequent v12 gate still converges.
    assert!(scenario.install(&[]));
    if let Some(bundle) = read_v11_bundle_path() {
        let v11_repeat = scenario.command_at_with(
            Some(&bundle),
            scenario.workspace(),
            ["install", "--no-frozen-lockfile"],
        );
        assert_success(&v11_repeat);
        let v11_records = ndjson_records(&v11_repeat);
        assert!(
            v11_records
                .iter()
                .any(|record| record["message"] == "Already up to date")
                && !v11_records
                    .iter()
                    .any(|record| record["name"] == "pnpm:stage"),
            "v11 repeat install must accept v12's recorded autoDedupe setting",
        );

        scenario.set_dependencies("a", &json!({ "@pnpm.e2e/bar": "100.0.0" }));
        let v11_filtered = scenario.command_at_with(
            Some(&bundle),
            scenario.workspace(),
            ["install", "--no-frozen-lockfile", "--filter", "a"],
        );
        assert_success(&v11_filtered);

        assert!(
            scenario.install(&["--filter", "a"]),
            "v12 filtered repeat install must take the fast path after v11 rewrote pnpm-state.yaml",
        );
        assert!(
            scenario.exec(&["a"], "error"),
            "v12 verifyDepsBeforeRun=error must pass after v11 rewrote pnpm-state.yaml",
        );
    }
}

#[test]
fn verify_deps_install_forwards_lockfile_dir_and_converges() {
    let layout = Layout { hoisted: false, shared_lockfile: true, inject_workspace_packages: false };
    run_across_drivers(layout, |layout| {
        let scenario = Scenario::new(layout);
        scenario.write_manifest(
            "a",
            json!({
                "dependencies": {
                    "@pnpm.e2e/foo": "100.0.0"
                },
                "scripts": {
                    "test": r#"node -e "require('@pnpm.e2e/foo/package.json')""#
                }
            }),
        );
        let project_dir = scenario
            .workspace()
            .join("packages")
            .join("a");
        let lockfile_dir = scenario.workspace().with_file_name("shared-lockfile-dir");
        let _ = fs::remove_dir_all(&lockfile_dir);
        fs::create_dir_all(&lockfile_dir).expect("create lockfile dir");
        let lockfile_dir = lockfile_dir.canonicalize().unwrap_or(lockfile_dir);

        let lockfile_dir_arg = format!("--lockfile-dir={}", lockfile_dir.display());
        let first_run = scenario.command_at(
            &project_dir,
            [lockfile_dir_arg.as_str(), "--config.verify-deps-before-run=install", "run", "test"],
        );
        assert_success(&first_run);
        assert!(
            lockfile_dir.join("pnpm-lock.yaml").is_file(),
            "spawned install must write pnpm-lock.yaml in --lockfile-dir",
        );
        assert!(
            !scenario
                .workspace()
                .join("pnpm-lock.yaml")
                .exists()
                && !project_dir.join("pnpm-lock.yaml").exists(),
            "spawned install must not write pnpm-lock.yaml in the workspace or project directory",
        );
        assert!(
            lockfile_dir
                .join("node_modules")
                .join(".pnpm-workspace-state-v1.json")
                .is_file(),
            "spawned install must write workspace state under --lockfile-dir",
        );

        let second_run = scenario.command_at(
            &project_dir,
            [lockfile_dir_arg.as_str(), "--config.verify-deps-before-run=error", "run", "test"],
        );
        assert_success(&second_run);
    });
}
