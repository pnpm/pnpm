use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path, write_project_manifest};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt, fs,
    path::{Path, PathBuf},
    process::Output,
};

pub(super) type Deps = &'static [(&'static str, &'static str)];

pub(super) struct Project {
    pub(super) dir: &'static str,
    pub(super) deps: Deps,
    pub(super) peers: Deps,
}

pub(super) const fn project(dir: &'static str, deps: Deps) -> Project {
    Project { dir, deps, peers: &[] }
}

/// A dependency of `project` whose specifier changes to `to`, so whatever it
/// provides as a peer moves to another version.
pub(super) struct Move {
    pub(super) project: &'static str,
    pub(super) alias: &'static str,
    pub(super) to: &'static str,
}

/// A sequence of installs whose lockfile must equal a fresh resolution's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Check {
    /// Adding the dependencies one install at a time.
    Incremental,
    /// Applying the [`Move`]s over the fresh lockfile.
    Move,
    /// Undoing the [`Move`]s over the moved lockfile.
    MoveBack,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Incremental => "adding one dependency at a time",
            Self::Move => "moving the providers over the lockfile",
            Self::MoveBack => "moving the providers back over the lockfile",
        })
    }
}

/// A known case where a [`Check`] leaves a lockfile that differs from a fresh
/// resolution. On every row it names, at least one [`lockfile_entries`] path
/// under its `paths` differs.
pub(super) struct Divergence {
    pub(super) issue: &'static str,
    pub(super) check: Check,
    pub(super) rows: fn(Row) -> bool,
    /// The divergence reproduces only through pnpm v11.
    pub(super) v11_only: bool,
    pub(super) paths: &'static [&'static str],
}

impl Divergence {
    fn owns(&self, entry: &str) -> bool {
        self.paths
            .iter()
            .any(|path| entry.starts_with(path))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    pub(super) auto_install_peers: bool,
    pub(super) dedupe_peer_dependents: bool,
    pub(super) inject_workspace_packages: bool,
    pub(super) auto_dedupe: bool,
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

/// The pnpm a [`Harness`] runs.
enum Cli {
    V12,
    /// `node` running the pnpm v11 bundle at this path.
    V11(OsString),
}

impl Cli {
    fn runs(&self, row: Row) -> bool {
        matches!(self, Self::V12) || !row.auto_dedupe
    }
}

pub(super) struct Harness {
    cli: Cli,
    fixture: WorkspaceFixture,
    base_yaml: String,
    projects: Vec<Project>,
    moves: Vec<Move>,
    divergences: Vec<Divergence>,
    violations: RefCell<Vec<String>>,
}

impl Harness {
    pub(super) fn check(projects: Vec<Project>, divergences: Vec<Divergence>) {
        Self::check_with_moves(projects, vec![], divergences);
    }

    pub(super) fn check_with_moves(
        projects: Vec<Project>,
        moves: Vec<Move>,
        divergences: Vec<Divergence>,
    ) {
        let v12 = Self::new(Cli::V12, projects, moves, divergences);
        let v12_fresh = v12.check_rows();
        let mut violations = v12.violations.into_inner();
        if let Some(bundle) = read_v11_bundle_path() {
            let v11 = Self::new(Cli::V11(bundle), v12.projects, v12.moves, v12.divergences);
            let v11_fresh = v11.check_rows();
            for (row, v11_lockfile) in &v11_fresh {
                if let Some(v12_lockfile) = v12_fresh.get(row)
                    && v12_lockfile != v11_lockfile
                {
                    v11.violation_at(
                        row,
                        &format!(
                            "a fresh v11 install differs from a fresh v12 install:\n{}",
                            diff(v12_lockfile, v11_lockfile),
                        ),
                        None,
                    );
                }
            }
            violations.extend(v11.violations.into_inner());
        }
        assert!(
            violations.is_empty(),
            "{} violations:\n\n{}",
            violations.len(),
            violations.join("\n\n"),
        );
    }

    fn new(
        cli: Cli,
        projects: Vec<Project>,
        moves: Vec<Move>,
        divergences: Vec<Divergence>,
    ) -> Self {
        let fixture = WorkspaceFixture::new();
        write_project_manifest(&fixture.workspace, "root", ManifestDeps::default());
        let base_yaml = fs::read_to_string(fixture.workspace.join("pnpm-workspace.yaml"))
            .expect("read pnpm-workspace.yaml");
        Self {
            cli,
            fixture,
            base_yaml,
            projects,
            moves,
            divergences,
            violations: RefCell::default(),
        }
    }

    /// Check every row the CLI runs, returning each row's fresh lockfile by
    /// the row's description.
    fn check_rows(&self) -> BTreeMap<String, String> {
        let with_workspace_deps = self.projects
            .iter()
            .flat_map(|project| project.deps)
            .any(|(_, spec)| spec.starts_with("workspace:"));
        Row::all(with_workspace_deps)
            .filter(|row| self.cli.runs(*row))
            .filter_map(|row| Some((row.to_string(), self.check_row(row)?)))
            .collect()
    }

    fn check_row(&self, row: Row) -> Option<String> {
        self.clean();
        fs::write(
            self.workspace().join("pnpm-workspace.yaml"),
            format!("{}{}", self.base_yaml, row.yaml()),
        )
        .expect("write pnpm-workspace.yaml");
        self.write_manifests(self.dep_count(), false);
        let fresh = self.install(row, "a fresh install")?;
        self.check_repeat(row, &fresh);
        self.check_run_gate(row);
        self.check_frozen(row, &fresh);
        self.check_dedupe(row);
        self.check_deploys(row);
        self.check_from_scratch(row, &fresh);
        self.check_incremental(row, &fresh);
        self.check_moves(row, &fresh);
        Some(fresh)
    }

    fn check_repeat(&self, row: Row, fresh: &str) {
        if let Some(repeat) = self.install(row, "a repeat install")
            && repeat != fresh
        {
            let what = format!("a repeat install changed the lockfile:\n{}", diff(fresh, &repeat));
            self.violation(row, &what, None);
        }
    }

    fn check_run_gate(&self, row: Row) {
        let output = self.run(
            self.workspace(),
            &["--config.verify-deps-before-run=error", "exec", "node", "-e", ""],
        );
        if !output.status.success() {
            self.violation(
                row,
                "verifyDepsBeforeRun=error failed right after a fresh install",
                Some(&output),
            );
        }
    }

    fn check_frozen(&self, row: Row, fresh: &str) {
        self.remove_node_modules();
        let output = self.run(self.workspace(), &["install", "--frozen-lockfile"]);
        if !output.status.success() {
            self.violation(row, "a frozen install rejected the fresh lockfile", Some(&output));
        } else if self.lockfile() != fresh {
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
            self.write_manifests(count, false);
            if self
                .install(row, &format!("installing {count} dependencies"))
                .is_none()
            {
                return;
            }
        }
        let incremental = self.lockfile();
        if self.compare(row, Check::Incremental, fresh, &incremental) {
            self.check_dedupe_converges(row, fresh);
        }
    }

    fn check_dedupe_converges(&self, row: Row, fresh: &str) {
        let output = self.run(self.workspace(), &["dedupe"]);
        if !output.status.success() {
            self.violation(row, "dedupe failed after incremental installs", Some(&output));
            return;
        }
        let deduped = self.lockfile();
        if deduped != fresh {
            let what = format!(
                "dedupe after incremental installs differs from a fresh install:\n{}",
                diff(fresh, &deduped),
            );
            self.violation(row, &what, None);
        }
    }

    fn check_moves(&self, row: Row, fresh: &str) {
        if self.moves.is_empty() {
            return;
        }
        self.clean();
        self.write_manifests(self.dep_count(), true);
        let Some(moved) = self.install(row, "a fresh install of the moved manifests") else {
            return;
        };
        self.clean();
        self.write_manifests(self.dep_count(), false);
        if self.install(row, "a fresh install before moving").is_none() {
            return;
        }
        for (check, moved_manifests, expected) in
            [(Check::Move, true, moved.as_str()), (Check::MoveBack, false, fresh)]
        {
            self.write_manifests(self.dep_count(), moved_manifests);
            let Some(actual) = self.install(row, &check.to_string()) else { return };
            self.compare(row, check, expected, &actual);
        }
    }

    /// Record a violation for every entry of `actual` that differs from
    /// `expected` and no [`Divergence`] of `check` on `row` owns, and for
    /// every such divergence that no longer reproduces. Whether anything
    /// differs.
    fn compare(&self, row: Row, check: Check, expected: &str, actual: &str) -> bool {
        let changed = changed_entries(expected, actual);
        let known: Vec<&Divergence> = self.divergences
            .iter()
            .filter(|divergence| {
                divergence.check == check
                    && (divergence.rows)(row)
                    && (!divergence.v11_only || matches!(self.cli, Cli::V11(_)))
            })
            .collect();
        for divergence in &known {
            if !changed
                .iter()
                .any(|(_, entry)| divergence.owns(entry))
            {
                let what = format!(
                    "the divergence tracked by {} in {check} no longer reproduces",
                    divergence.issue,
                );
                self.violation(row, &what, None);
            }
        }
        let unexplained: Vec<String> = changed
            .iter()
            .filter(|(_, entry)| {
                !known
                    .iter()
                    .any(|divergence| divergence.owns(entry))
            })
            .map(|(sign, entry)| format!("{sign} {entry}"))
            .collect();
        if !unexplained.is_empty() {
            let what = format!("{check} differs from a fresh install:\n{}", unexplained.join("\n"));
            self.violation(row, &what, None);
        }
        !changed.is_empty()
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

    /// Every command the harness runs writes a lockfile when it succeeds.
    fn lockfile(&self) -> String {
        fs::read_to_string(self.workspace().join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml")
    }

    fn dep_count(&self) -> usize {
        self.projects
            .iter()
            .map(|project| project.deps.len())
            .sum()
    }

    /// Write the manifests with the first `count` dependencies of the
    /// fixture, counted across projects in fixture order, with the
    /// [`Move`]s applied when `moved`.
    fn write_manifests(&self, count: usize, moved: bool) {
        let mut remaining = count;
        for project in &self.projects {
            let take = remaining.min(project.deps.len());
            remaining -= take;
            let prod: Vec<(&str, &str)> = project.deps[..take]
                .iter()
                .map(|&(alias, spec)| {
                    (alias, if moved { self.moved_spec(project, alias, spec) } else { spec })
                })
                .collect();
            let deps = ManifestDeps { prod: &prod, peer: project.peers, ..ManifestDeps::default() };
            write_project_manifest(&self.project_dir(project), project.dir, deps);
        }
    }

    fn moved_spec(&self, project: &Project, alias: &str, spec: &'static str) -> &'static str {
        self.moves
            .iter()
            .find(|change| change.project == project.dir && change.alias == alias)
            .map_or(spec, |change| change.to)
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
        let Cli::V11(bundle) = &self.cli else {
            return self.fixture.command_at(cwd, args);
        };
        self.fixture.v11_command_at(bundle, cwd, args)
    }

    /// `install` over whatever lockfile is there, returning the lockfile it
    /// writes.
    fn install(&self, row: Row, what: &str) -> Option<String> {
        let output = self.run(self.workspace(), &["install", "--no-frozen-lockfile"]);
        if output.status.success() {
            return Some(self.lockfile());
        }
        self.violation(row, &format!("{what} failed"), Some(&output));
        None
    }

    fn violation(&self, row: Row, what: &str, output: Option<&Output>) {
        self.violation_at(&row.to_string(), what, output);
    }

    fn violation_at(&self, row: &str, what: &str, output: Option<&Output>) {
        let detail = output.map_or_else(String::new, |output| {
            format!(
                "\n--- stdout\n{}\n--- stderr\n{}",
                last_lines(&String::from_utf8_lossy(&output.stdout)),
                last_lines(&String::from_utf8_lossy(&output.stderr)),
            )
        });
        let cli = if matches!(self.cli, Cli::V11(_)) { "v11 " } else { "" };
        self.violations
            .borrow_mut()
            .push(format!("[{cli}{row}] {what}{detail}"));
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

fn diff(expected: &str, actual: &str) -> String {
    changed_entries(expected, actual)
        .iter()
        .map(|(sign, entry)| format!("{sign} {entry}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The [`lockfile_entries`] only one side has, `-` for `expected` and `+`
/// for `actual`.
fn changed_entries(expected: &str, actual: &str) -> Vec<(char, String)> {
    let expected = lockfile_entries(expected);
    let actual = lockfile_entries(actual);
    let removed = expected
        .difference(&actual)
        .map(|entry| ('-', entry.clone()));
    let added = actual
        .difference(&expected)
        .map(|entry| ('+', entry.clone()));
    removed.chain(added).collect()
}

/// Every leaf of `lockfile` as `key > key > ... = value`.
fn lockfile_entries(lockfile: &str) -> BTreeSet<String> {
    let value: Value = serde_saphyr::from_str(lockfile).expect("parse a lockfile");
    let mut entries = BTreeSet::new();
    collect_entries(&value, "", &mut entries);
    entries
}

fn collect_entries(value: &Value, path: &str, entries: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let child_path =
                    if path.is_empty() { key.clone() } else { format!("{path} > {key}") };
                collect_entries(child, &child_path, entries);
            }
        }
        _ => {
            entries.insert(format!("{path} = {value}"));
        }
    }
}
