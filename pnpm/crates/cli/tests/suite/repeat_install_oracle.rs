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
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use pnpm_lockfile::ProjectSnapshot;
use serde_json::{Value, json};

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
    fixture: WorkspaceFixture,
    layout: Layout,
    oracle_runs: usize,
}

/// What an install leaves behind that a user can observe: where every
/// dependency resolves from each project, and every lockfile.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    tree: BTreeMap<String, String>,
    lockfiles: BTreeMap<String, String>,
}

/// Where the selected projects' dependencies resolve from, and the entries
/// that record them in the wanted and the current lockfiles.
type SelectionView = (BTreeMap<String, String>, BTreeMap<String, [Option<ProjectSnapshot>; 2]>);

impl Scenario {
    /// A workspace whose `c` depends on its sibling `a`, so a selection reaches
    /// a project it did not name, and whose `b` has a dev dependency.
    fn new(layout: Layout) -> Self {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(&layout.workspace_yaml());
        fixture.write_root_manifest("root", ManifestDeps::default());
        let scenario = Self { fixture, layout, oracle_runs: 0 };
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

    /// `pnpm install` with `args`, returning whether it took the fast path:
    /// it said "Already up to date" without starting a resolution or an
    /// import.
    fn install(&self, args: &[&str]) -> bool {
        let output = self.fixture.command_at(
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
        let mut args = vec![format!("--config.verify-deps-before-run={action}")];
        for project in selection {
            args.push("--filter".to_string());
            args.push((*project).to_string());
        }
        args.extend(["exec", "node", "-e", ""].map(str::to_string));
        let output = self.fixture.command_at(self.workspace(), &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("left node_modules out of date"),
            "{:?}: the install the gate spawned for {selection:?} did not converge:\n{stderr}",
            self.layout,
        );
        output.status.success()
    }

    /// [`Self::exec`] without a filter, from `project` (`.` for the root).
    fn exec_from(&self, project: &str, action: &str) -> bool {
        self.fixture
            .command_at(
                &self.workspace().join(project),
                [&*format!("--config.verify-deps-before-run={action}"), "exec", "node", "-e", ""],
            )
            .status
            .success()
    }

    /// Drop the validation records from every state, as pnpm v11 writes it.
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
        let root = fs::canonicalize(workspace).expect("canonicalize workspace");
        let mut resolver = TreeView { root, memo: HashMap::new() };
        projects
            .iter()
            .map(|project| {
                let dir = workspace.join("packages").join(project);
                ((*project).to_string(), resolver.project(&dir, with_dev))
            })
            .collect()
    }

    /// Every lockfile under `workspace`, the current ones included.
    fn lockfiles(workspace: &Path) -> BTreeMap<String, String> {
        walk_files(workspace)
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name == "pnpm-lock.yaml" || name == "lock.yaml")
            })
            .map(|path| {
                let relative = path.strip_prefix(workspace).expect("under the workspace");
                let text = fs::read_to_string(&path).expect("read a lockfile");
                (relative.to_string_lossy().replace('\\', "/"), text)
            })
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tree: Self::tree(self.workspace(), &PROJECTS, true),
            lockfiles: Self::lockfiles(self.workspace()),
        }
    }

    /// The entries that record `projects` in the wanted and the current
    /// lockfiles.
    fn importers(
        &self,
        workspace: &Path,
        projects: &[&str],
    ) -> BTreeMap<String, [Option<ProjectSnapshot>; 2]> {
        let entry = |lockfile: PathBuf, importer_id: &str| {
            lockfile
                .is_file()
                .then(|| read_lockfile(&lockfile))
                .and_then(|lockfile| lockfile.importers.get(importer_id).cloned())
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

    /// [`Self::selection_view`] of a clean install of a copy of the sources.
    fn oracle(&mut self, selection: &[&str]) -> SelectionView {
        self.oracle_runs += 1;
        let copy = self
            .workspace()
            .with_file_name(format!("oracle-{}", self.oracle_runs));
        copy_sources(self.workspace(), &copy);
        let mut args = vec!["install", "--no-frozen-lockfile"];
        if self.recorded_production_only() {
            args.push("--prod");
        }
        let output = self.fixture.command_at(&copy, args);
        assert_success(&output);
        self.selection_view(&copy, selection)
    }

    /// Whether the last install left the dev dependencies out, so a run gate
    /// holds the tree to a production install.
    fn recorded_production_only(&self) -> bool {
        let state_dir = if self.layout.shared_lockfile {
            self.workspace().to_path_buf()
        } else {
            self.workspace().join("packages/a")
        };
        fs::read_to_string(state_dir.join("node_modules/.pnpm-workspace-state-v1.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|state| state["settings"]["dev"] == false)
    }

    /// The run gate for `selection` lets the command through exactly when
    /// the selection already is what a clean install makes it.
    #[track_caller]
    fn assert_run_gate(&mut self, selection: &[&str]) {
        let passed = self.exec(selection, "error");
        let actual = self.selection_view(self.workspace(), selection);
        let expected = self.oracle(selection);
        let current = actual == expected;
        assert!(
            !passed || current,
            "{:?}: the run gate for {selection:?} let the command run, but a clean install \
             differs:\n  actual: {actual:#?}\n  expected: {expected:#?}",
            self.layout,
        );
        assert!(
            passed || !current,
            "{:?}: the run gate for {selection:?} refused, but a clean install changes nothing \
             for it",
            self.layout,
        );
    }

    /// `pnpm install` takes its fast path exactly when a full install changes
    /// nothing, and takes it on the next run. `conservative` allows a refusal
    /// for a tree a full install leaves as it is.
    #[track_caller]
    fn assert_install_gate(&self, conservative: bool) {
        let before = self.snapshot();
        if self.install(&[]) {
            assert!(!self.install(&["--config.optimistic-repeat-install=false"]));
            assert_eq!(
                self.snapshot(),
                before,
                "{:?}: the install fast path was taken, but a full install changes the tree",
                self.layout,
            );
        } else if !conservative {
            assert_ne!(
                self.snapshot(),
                before,
                "{:?}: the install fast path was refused, but the install changed nothing",
                self.layout,
            );
        }
        assert!(self.install(&[]), "{:?}: a repeat install must take the fast path", self.layout);
    }

    /// Every gate over the workspace, then the convergence the install leaves.
    #[track_caller]
    fn check_gates(&mut self) {
        self.check_gates_allowing(false);
    }

    #[track_caller]
    fn check_gates_allowing(&mut self, conservative: bool) {
        for selection in [&["a"][..], &["b"], &["c"], &PROJECTS] {
            self.assert_run_gate(selection);
        }
        self.assert_install_gate(conservative);
        for selection in [&["a"][..], &["b"], &["c"], &PROJECTS] {
            assert!(
                self.exec(selection, "error"),
                "{:?}: the run gate for {selection:?} must pass right after a full install",
                self.layout,
            );
        }
    }

    /// `exec` under the `install` action, then the properties that action
    /// promises: the command ran, and the gate passes next time.
    #[track_caller]
    fn exec_installing(&self, selection: &[&str]) {
        assert!(self.exec(selection, "install"), "the gated command must run");
        assert!(
            self.exec(selection, "error"),
            "{:?}: the install the gate spawned for {selection:?} must satisfy the gate",
            self.layout,
        );
    }
}

/// Resolves dependencies through `node_modules` the way Node.js does, from
/// each package's real path, and spells the result without absolute paths.
struct TreeView {
    root: PathBuf,
    memo: HashMap<PathBuf, String>,
}

impl TreeView {
    fn project(&mut self, project_dir: &Path, with_dev: bool) -> String {
        let manifest = read_manifest(project_dir);
        let groups: &[&str] = if with_dev {
            &["dependencies", "optionalDependencies", "devDependencies"]
        } else {
            &["dependencies", "optionalDependencies"]
        };
        let dependencies = declared(&manifest, groups);
        self.dependencies(project_dir, &dependencies)
    }

    fn dependencies(&mut self, from: &Path, names: &[String]) -> String {
        let resolved = names
            .iter()
            .map(|name| {
                let target = resolve(from, name)
                    .map_or_else(|| "<missing>".to_string(), |dir| self.package(&dir));
                format!("{name}={target}")
            })
            .collect::<Vec<_>>();
        format!("{{{}}}", resolved.join(","))
    }

    fn package(&mut self, dir: &Path) -> String {
        let real = fs::canonicalize(dir).expect("canonicalize a resolved dependency");
        if let Some(seen) = self.memo.get(&real) {
            return seen.clone();
        }
        self.memo.insert(real.clone(), "<cycle>".to_string());
        let manifest = read_manifest(&real);
        let label = match real.strip_prefix(&self.root) {
            Ok(relative)
                if !relative
                    .components()
                    .any(|part| part.as_os_str() == "node_modules") =>
            {
                format!("workspace:{}", relative.to_string_lossy().replace('\\', "/"))
            }
            _ => format!(
                "{}@{}",
                manifest["name"].as_str().unwrap_or_default(),
                manifest["version"].as_str().unwrap_or_default(),
            ),
        };
        let dependencies = declared(&manifest, &["dependencies", "optionalDependencies"]);
        let described = format!("{label}{}", self.dependencies(&real, &dependencies));
        self.memo.insert(real, described.clone());
        described
    }
}

fn read_manifest(dir: &Path) -> Value {
    fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn declared(manifest: &Value, groups: &[&str]) -> Vec<String> {
    let mut names = groups
        .iter()
        .filter_map(|group| manifest[*group].as_object())
        .flat_map(|deps| deps.keys().cloned())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Node.js's lookup of `name` from a package at `from`.
fn resolve(from: &Path, name: &str) -> Option<PathBuf> {
    from.ancestors()
        .filter(|dir| {
            dir.file_name()
                .is_none_or(|base| base != "node_modules")
        })
        .map(|dir| dir.join("node_modules").join(name))
        .find(|candidate| candidate.join("package.json").is_file())
}

/// Every regular file under `root`, without following links.
fn walk_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk the workspace"))
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// Copy everything under `from` except the `node_modules` directories.
fn copy_sources(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "node_modules")
    {
        let entry = entry.expect("walk the workspace");
        let target = to.join(
            entry
                .path()
                .strip_prefix(from)
                .expect("under the workspace"),
        );
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).expect("create a directory in the copy");
        } else {
            fs::copy(entry.path(), &target).expect("copy a source file");
        }
    }
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
        scenario.install(&["--filter", "c"]),
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
        scenario.install(&[]),
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
    assert!(scenario.install(&[]), "{layout:?}: unchanged inputs keep the fast path");
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

/// A state without validation records, as pnpm v11 writes it, is proven again
/// from the lockfiles once and then recorded. After a filtered install the
/// install gate refuses it outright: pnpm v11's current lockfile cannot show
/// which projects that install left out.
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
                super::$scenario(LAYOUT);
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
