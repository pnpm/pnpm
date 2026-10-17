//! Peer fixtures installed through every path that binds peers, across the
//! settings that change how peers bind. For each fixture and settings row,
//! starting from a fresh resolution of the manifests:
//!
//! - a frozen install accepts the fresh lockfile and leaves it unchanged,
//! - `dedupe --check` passes,
//! - every project deploys, and its deployed lockfile installs frozen,
//! - a second resolution from scratch writes the same lockfile,
//! - adding the dependencies one install at a time writes the same lockfile,
//!   unless a [`Divergence`] of the fixture names the row,
//! - `dedupe` after those installs writes the same lockfile.
//!
//! Every violation of a fixture is collected before the test fails, so one
//! run reports the whole matrix.

use crate::_utils::{ManifestDeps, WorkspaceFixture, write_project_manifest};
use std::{
    cell::RefCell,
    fmt, fs,
    path::{Path, PathBuf},
    process::Output,
};

type Deps = &'static [(&'static str, &'static str)];

struct Project {
    dir: &'static str,
    deps: Deps,
    peers: Deps,
}

const fn project(dir: &'static str, deps: Deps) -> Project {
    Project { dir, deps, peers: &[] }
}

/// A known case where adding the dependencies one install at a time leaves a
/// lockfile that differs from a fresh resolution.
struct Divergence {
    issue: &'static str,
    rows: fn(Row) -> bool,
}

#[derive(Debug, Clone, Copy)]
struct Row {
    auto_install_peers: bool,
    dedupe_peer_dependents: bool,
    inject_workspace_packages: bool,
    auto_dedupe: bool,
}

impl Row {
    /// Every combination of the settings. `injectWorkspacePackages` varies
    /// only for fixtures that depend on a workspace package.
    fn all(with_workspace_deps: bool) -> impl Iterator<Item = Self> {
        (0..16u8)
            .map(|bits| Self {
                auto_install_peers: bits & 1 != 0,
                dedupe_peer_dependents: bits & 2 != 0,
                inject_workspace_packages: bits & 4 != 0,
                auto_dedupe: bits & 8 != 0,
            })
            .filter(move |row| with_workspace_deps || !row.inject_workspace_packages)
    }

    fn yaml(self) -> String {
        format!(
            "autoInstallPeers: {}\ndedupePeerDependents: {}\ninjectWorkspacePackages: {}\nautoDedupe: {}\n",
            self.auto_install_peers,
            self.dedupe_peer_dependents,
            self.inject_workspace_packages,
            self.auto_dedupe,
        )
    }
}

impl fmt::Display for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "autoInstallPeers={} dedupePeerDependents={} injectWorkspacePackages={} autoDedupe={}",
            self.auto_install_peers,
            self.dedupe_peer_dependents,
            self.inject_workspace_packages,
            self.auto_dedupe,
        )
    }
}

struct Harness {
    fixture: WorkspaceFixture,
    base_yaml: String,
    projects: Vec<Project>,
    divergences: Vec<Divergence>,
    violations: RefCell<Vec<String>>,
}

impl Harness {
    fn check(projects: Vec<Project>, divergences: Vec<Divergence>) {
        let with_workspace_deps = projects
            .iter()
            .flat_map(|project| project.deps)
            .any(|(_, spec)| spec.starts_with("workspace:"));
        let fixture = WorkspaceFixture::new();
        write_project_manifest(&fixture.workspace, "root", ManifestDeps::default());
        let base_yaml = fs::read_to_string(fixture.workspace.join("pnpm-workspace.yaml"))
            .expect("read pnpm-workspace.yaml");
        let harness =
            Self { fixture, base_yaml, projects, divergences, violations: RefCell::default() };
        for row in Row::all(with_workspace_deps) {
            harness.check_row(row);
        }
        let violations = harness.violations.into_inner();
        assert!(
            violations.is_empty(),
            "{} violations:\n\n{}",
            violations.len(),
            violations.join("\n\n"),
        );
    }

    fn check_row(&self, row: Row) {
        self.clean();
        fs::write(
            self.workspace().join("pnpm-workspace.yaml"),
            format!("{}{}", self.base_yaml, row.yaml()),
        )
        .expect("write pnpm-workspace.yaml");
        self.write_manifests(self.dep_count());
        let Some(fresh) = self.install(row, "a fresh install") else { return };
        self.check_frozen(row, &fresh);
        self.check_dedupe(row);
        self.check_deploys(row);
        self.check_from_scratch(row, &fresh);
        self.check_incremental(row, &fresh);
    }

    fn check_frozen(&self, row: Row, fresh: &str) {
        self.remove_node_modules();
        let output = self.run(self.workspace(), &["install", "--frozen-lockfile"]);
        if !output.status.success() {
            self.violation(row, "a frozen install rejected the fresh lockfile", Some(&output));
        } else if self.lockfile().as_deref() != Some(fresh) {
            self.violation(row, "a frozen install changed the fresh lockfile", None);
        }
    }

    fn check_dedupe(&self, row: Row) {
        let output = self.run(self.workspace(), &["dedupe", "--check"]);
        if !output.status.success() {
            self.violation(row, "dedupe --check failed after a fresh install", Some(&output));
        }
    }

    fn check_deploys(&self, row: Row) {
        for project in &self.projects {
            let deploy_dir = self.deploy_dir(project);
            let output = self.run(
                self.workspace(),
                &["--filter", project.dir, "deploy", &deploy_dir.to_string_lossy()],
            );
            if !output.status.success() {
                self.violation(row, &format!("deploying {} failed", project.dir), Some(&output));
                continue;
            }
            fs::copy(self.workspace().join(".npmrc"), deploy_dir.join(".npmrc"))
                .expect("copy .npmrc into the deploy directory");
            remove_dir(&deploy_dir.join("node_modules"));
            let output = self.run(&deploy_dir, &["install", "--frozen-lockfile"]);
            if !output.status.success() {
                let what = format!("a frozen install of the deployed {} failed", project.dir);
                self.violation(row, &what, Some(&output));
            }
        }
    }

    fn check_from_scratch(&self, row: Row, fresh: &str) {
        self.clean();
        if let Some(again) = self.install(row, "a second fresh install")
            && again != fresh
        {
            let what = format!("a second resolution differs:\n{}", diff(fresh, &again));
            self.violation(row, &what, None);
        }
    }

    fn check_incremental(&self, row: Row, fresh: &str) {
        self.clean();
        for count in 0..=self.dep_count() {
            self.write_manifests(count);
            if self
                .install(row, &format!("installing {count} dependencies"))
                .is_none()
            {
                return;
            }
        }
        let incremental = self.lockfile().unwrap_or_default();
        let known = self.divergences
            .iter()
            .find(|divergence| (divergence.rows)(row));
        match (incremental == fresh, known) {
            (true, Some(divergence)) => self.violation(
                row,
                &format!("the divergence tracked by {} no longer reproduces", divergence.issue),
                None,
            ),
            (false, None) => {
                let what = format!(
                    "adding one dependency at a time differs:\n{}",
                    diff(fresh, &incremental),
                );
                self.violation(row, &what, None);
            }
            _ => {}
        }
        if incremental != fresh {
            self.check_dedupe_converges(row, fresh);
        }
    }

    fn check_dedupe_converges(&self, row: Row, fresh: &str) {
        let output = self.run(self.workspace(), &["dedupe"]);
        if !output.status.success() {
            self.violation(row, "dedupe failed after incremental installs", Some(&output));
        } else if let Some(deduped) = self.lockfile()
            && deduped != fresh
        {
            let what = format!(
                "dedupe after incremental installs differs from a fresh install:\n{}",
                diff(fresh, &deduped),
            );
            self.violation(row, &what, None);
        }
    }

    fn workspace(&self) -> &Path {
        &self.fixture.workspace
    }

    fn project_dir(&self, project: &Project) -> PathBuf {
        self.workspace()
            .join("packages")
            .join(project.dir)
    }

    /// Next to the workspace, so the relative store and cache paths in the
    /// copied `.npmrc` resolve to the same directories.
    fn deploy_dir(&self, project: &Project) -> PathBuf {
        self.workspace()
            .parent()
            .expect("the workspace has a parent")
            .join(format!("deploy-{}", project.dir))
    }

    fn lockfile(&self) -> Option<String> {
        fs::read_to_string(self.workspace().join("pnpm-lock.yaml")).ok()
    }

    fn dep_count(&self) -> usize {
        self.projects
            .iter()
            .map(|project| project.deps.len())
            .sum()
    }

    /// Write the manifests with the first `count` dependencies of the
    /// fixture, counted across projects in fixture order.
    fn write_manifests(&self, count: usize) {
        let mut remaining = count;
        for project in &self.projects {
            let take = remaining.min(project.deps.len());
            remaining -= take;
            let deps = ManifestDeps {
                prod: &project.deps[..take],
                peer: project.peers,
                ..ManifestDeps::default()
            };
            write_project_manifest(&self.project_dir(project), project.dir, deps);
        }
    }

    fn remove_node_modules(&self) {
        remove_dir(&self.workspace().join("node_modules"));
        for project in &self.projects {
            remove_dir(&self.project_dir(project).join("node_modules"));
        }
    }

    fn clean(&self) {
        self.remove_node_modules();
        let lockfile = self.workspace().join("pnpm-lock.yaml");
        if lockfile.exists() {
            fs::remove_file(lockfile).expect("remove the lockfile");
        }
        for project in &self.projects {
            remove_dir(&self.deploy_dir(project));
        }
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        self.fixture.command_at(cwd, args)
    }

    /// `install` over whatever lockfile is there, returning the lockfile it
    /// writes.
    fn install(&self, row: Row, what: &str) -> Option<String> {
        let output = self.run(self.workspace(), &["install", "--no-frozen-lockfile"]);
        if output.status.success() {
            return self.lockfile();
        }
        self.violation(row, &format!("{what} failed"), Some(&output));
        None
    }

    fn violation(&self, row: Row, what: &str, output: Option<&Output>) {
        let detail = output.map_or_else(String::new, |output| {
            format!(
                "\n--- stdout\n{}\n--- stderr\n{}",
                last_lines(&String::from_utf8_lossy(&output.stdout)),
                last_lines(&String::from_utf8_lossy(&output.stderr)),
            )
        });
        self.violations
            .borrow_mut()
            .push(format!("[{row}] {what}{detail}"));
    }
}

fn remove_dir(dir: &Path) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("remove a directory");
    }
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(15)..].join("\n")
}

/// The lines only one side has, which is enough to point at a binding.
fn diff(expected: &str, actual: &str) -> String {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let removed = expected
        .iter()
        .filter(|line| !actual.contains(line))
        .map(|line| format!("-{line}"));
    let added = actual
        .iter()
        .filter(|line| !expected.contains(line))
        .map(|line| format!("+{line}"));
    removed
        .chain(added)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn satisfied_peer() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.0.0")],
        )],
        vec![],
    );
}

#[test]
fn missing_peer() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/has-foo100-peer", "1.0.0")])], vec![]);
}

#[test]
fn several_peers() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/abc", "1.0.0"),
                ("@pnpm.e2e/peer-a", "1.0.0"),
                ("@pnpm.e2e/peer-b", "1.0.0"),
                ("@pnpm.e2e/peer-c", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn optional_peers_partly_provided() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/abc-optional-peers", "1.0.0"), ("@pnpm.e2e/peer-a", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn optional_peer_also_in_dependencies() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/has-optional-peer-also-in-deps", "1.0.0"),
                ("@pnpm.e2e/bravo-dep", "1.0.0"),
            ],
        )],
        vec![Divergence { issue: OPTIONAL_PEER_ALSO_IN_DEPS_ISSUE, rows: |row| !row.auto_dedupe }],
    );
}

#[test]
fn two_package_peer_cycle() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/circular-peer-a", "1.0.0"), ("@pnpm.e2e/circular-peer-b", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn three_package_peer_cycle() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/circular-peers-1-of-3", "1.0.0"),
                ("@pnpm.e2e/circular-peers-2-of-3", "1.0.0"),
                ("@pnpm.e2e/circular-peers-3-of-3", "1.0.0"),
                ("@pnpm.e2e/peer-a", "1.0.0"),
                ("@pnpm.e2e/peer-b", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn complex_peer_cycle() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/complex-circular-peers-a", "1.0.0"),
                ("@pnpm.e2e/complex-circular-peers-b", "1.0.0"),
                ("@pnpm.e2e/complex-circular-peers-c", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn peer_cycle_with_a_shared_peer() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/closure-lib-a", "1.0.0"),
                ("@pnpm.e2e/closure-lib-b", "1.0.0"),
                ("@pnpm.e2e/closure-peer-x", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn peers_that_are_also_dependencies() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/deadlock.a", "1.0.0")])], vec![]);
}

#[test]
fn peer_diamond() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/peer-diamond-bundle", "1.0.0"), ("@pnpm.e2e/peer-diamond-ts", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn peer_of_itself() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/peer-of-itself", "1.0.0")])], vec![]);
}

#[test]
fn peer_resolved_from_own_dependencies() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/pkg-with-resolved-peer", "1.0.0"), ("@pnpm.e2e/peer-b", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn peer_of_a_peer() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/has-has-y-peer-peer", "1.0.0"),
                ("@pnpm.e2e/has-y-peer", "1.0.0"),
                ("@pnpm/y", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn projects_with_different_providers() {
    Harness::check(
        vec![
            project("a", &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.0.0")]),
            project("b", &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.1.0")]),
        ],
        vec![],
    );
}

#[test]
fn projects_with_conflicting_peer_ranges() {
    Harness::check(
        vec![
            project("a", &[("@pnpm.e2e/wants-peer-c-1", "1.0.0")]),
            project("b", &[("@pnpm.e2e/wants-peer-c-2", "1.0.0")]),
        ],
        vec![],
    );
}

#[test]
fn workspace_package_with_a_peer() {
    Harness::check(
        vec![
            Project { dir: "lib", deps: &[], peers: &[("@pnpm.e2e/foo", "100")] },
            project("a", &[("lib", "workspace:*"), ("@pnpm.e2e/foo", "100.0.0")]),
            project("b", &[("lib", "workspace:*"), ("@pnpm.e2e/foo", "100.1.0")]),
        ],
        vec![
            Divergence {
                issue: INJECTED_BEFORE_PROVIDER_ISSUE,
                rows: |row| row.inject_workspace_packages,
            },
            Divergence {
                issue: AUTO_INSTALLED_PEER_MOVES_ISSUE,
                rows: |row| row.auto_install_peers && !row.auto_dedupe,
            },
        ],
    );
}

const OPTIONAL_PEER_ALSO_IN_DEPS_ISSUE: &str = "https://github.com/pnpm/pnpm/issues/17056";
const INJECTED_BEFORE_PROVIDER_ISSUE: &str = "https://github.com/pnpm/pnpm/issues/17057";
const AUTO_INSTALLED_PEER_MOVES_ISSUE: &str = "https://github.com/pnpm/pnpm/issues/17058";
