use crate::{
    cli_args::{BenchmarkScenario, HyperfineOptions},
    fixtures::{LOCKFILE, PACKAGE_JSON},
    verify::executor,
    workspace_manifest::MinimalWorkspaceManifest,
};
use itertools::Itertools;
use os_display::Quotable;
use pacquet_fs::file_mode::make_file_executable;
use pipe_trait::Pipe;
use std::{
    borrow::Cow,
    fmt,
    fs::{self, File},
    io::Write,
    iter,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug)]
pub struct WorkEnv {
    pub root: PathBuf,
    pub with_pnpm: bool,
    pub revisions: Vec<String>,
    pub registry: String,
    pub repository: PathBuf,
    pub scenario: Option<BenchmarkScenario>,
    pub hyperfine_options: HyperfineOptions,
    pub fixture_dir: Option<PathBuf>,
}

impl WorkEnv {
    const INIT_PROXY_CACHE: BenchId<'static> = BenchId::Static(".init-proxy-cache");
    const PNPM: BenchId<'static> = BenchId::Static("pnpm");

    fn root(&self) -> &'_ Path {
        &self.root
    }

    fn revision_names(&self) -> impl Iterator<Item = &'_ str> + '_ {
        self.revisions.iter().map(AsRef::as_ref)
    }

    fn revision_ids(&self) -> impl Iterator<Item = BenchId<'_>> + '_ {
        self.revision_names().map(BenchId::PacquetRevision)
    }

    fn registry(&self) -> &'_ str {
        &self.registry
    }

    fn repository(&self) -> &'_ Path {
        &self.repository
    }

    fn bench_dir(&self, id: BenchId) -> PathBuf {
        self.root().join(id.to_string())
    }

    fn script_path(&self, id: BenchId) -> PathBuf {
        self.bench_dir(id).join("install.bash")
    }

    fn bash_command(&self, id: BenchId) -> String {
        let script_path = self.script_path(id);
        let script_path = script_path.to_str().expect("convert script path to UTF-8");
        format!("bash {script_path}")
    }

    fn revision_repo(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PacquetRevision(revision)).join("pacquet")
    }

    fn resolve_revision(&self, revision: &str) -> String {
        let output = Command::new("git")
            .current_dir(self.repository())
            .arg("rev-parse")
            .arg(revision)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .expect("git rev-parse");
        assert!(output.status.success());
        output
            .stdout
            .pipe(String::from_utf8)
            .expect("output of rev-parse is valid UTF-8")
            .trim()
            .to_string()
    }

    fn init(&self) {
        let scenario = self.scenario.expect("scenario set when init() is reached");
        eprintln!("Initializing...");
        let id_list = self
            .revision_ids()
            .chain(iter::once(WorkEnv::INIT_PROXY_CACHE))
            .chain(self.with_pnpm.then_some(WorkEnv::PNPM));
        for id in id_list {
            eprintln!("ID: {id}");
            let dir = self.bench_dir(id);
            let for_pnpm = matches!(id, BenchId::Static(_));
            fs::create_dir_all(&dir).expect("create directory for the revision");
            create_package_json(&dir, self.fixture_dir.as_deref());
            create_pnpm_workspace(&dir, self.fixture_dir.as_deref(), self.registry(), scenario);
            create_install_script(&dir, scenario, for_pnpm);
            create_npmrc(&dir, self.registry(), scenario);
            may_create_lockfile(&dir, scenario, self.fixture_dir.as_deref());
        }

        eprintln!("Populating proxy registry cache...");
        Command::new("bash")
            .arg(self.script_path(WorkEnv::INIT_PROXY_CACHE))
            .pipe_mut(executor("install.bash"))
    }

    pub fn build(&self) {
        eprintln!("Building...");
        for revision in self.revision_names() {
            eprintln!("Revision: {revision:?}");

            let repository = self.repository();
            let revision_repo = self.revision_repo(revision);

            // Resolve the revision against the source repository *before*
            // fetching, so the fetch can request the exact commit. A bare
            // `git fetch <repo>` only writes the source's `HEAD` to
            // `FETCH_HEAD`, which means a SHA that isn't reachable from
            // the source's HEAD (e.g. tip of `main` when the runner is on
            // a PR branch that's behind `main`) won't end up in the
            // bench-repo and the subsequent `git checkout <sha>` panics
            // with `unable to read tree`. See PR #321 comment
            // <https://github.com/pnpm/pacquet/pull/321#issuecomment-4326141435>.
            let commit = self.resolve_revision(revision);
            eprintln!("Resolved {revision:?} to {commit}");

            if revision_repo.exists() {
                if !revision_repo.join(".git").exists() {
                    eprintln!("Initializing a git repository at {revision_repo:?}...");
                    Command::new("git")
                        .current_dir(&revision_repo)
                        .arg("init")
                        .arg(&revision_repo)
                        .arg("--initial-branch=__blank__")
                        .pipe(executor("git init"));
                }

                eprintln!("Fetching {commit} from {repository:?}...");
                Command::new("git")
                    .current_dir(&revision_repo)
                    .arg("fetch")
                    .arg(repository)
                    .arg(&commit)
                    .pipe(executor("git fetch"));
            } else {
                eprintln!("Cloning {repository:?} to {revision_repo:?}...");
                Command::new("git")
                    .arg("clone")
                    .arg("--no-checkout")
                    .arg(repository)
                    .arg(&revision_repo)
                    .pipe(executor("git clone"));
            }

            eprintln!("Checking out {commit:?}...");
            Command::new("git")
                .current_dir(&revision_repo)
                .arg("checkout")
                .arg(&commit)
                .pipe(executor("git checkout"));

            eprintln!("List of branches:");
            Command::new("git")
                .current_dir(&revision_repo)
                .arg("branch")
                .pipe(executor("git branch"));

            eprintln!("Building {revision:?}...");
            Command::new("cargo")
                .current_dir(&revision_repo)
                .arg("build")
                .arg("--release")
                .arg("--bin=pacquet")
                .pipe(executor("cargo build"));
        }
    }

    fn benchmark(&self) {
        // Pre-benchmark wipe of `node_modules` *and* `store-dir` for
        // every benchmark target, regardless of scenario. The
        // hot-cache scenario's per-iteration `--prepare` intentionally
        // preserves `store-dir` so subsequent iterations can reuse
        // it, which means whatever a previous run / scenario / partial
        // invocation left in `store-dir` would otherwise carry into
        // the warmup — and the warmup wouldn't actually be what
        // primes the store. Wiping once upfront makes the warmup the
        // priming run no matter what state the work-env was in. For
        // cold-cache scenarios this is redundant with the per-iteration
        // wipe but harmless (Copilot review on #296).
        for dir in self
            .revision_ids()
            .chain(self.with_pnpm.then_some(WorkEnv::PNPM))
            .map(|id| self.bench_dir(id))
        {
            for name in ["node_modules", "store-dir"] {
                let path = dir.join(name);
                if path.exists() {
                    fs::remove_dir_all(&path).expect("pre-benchmark wipe");
                }
            }
        }

        // hyperfine runs `--prepare` before *each* timed invocation, so
        // cleanup must cover every bench dir we're about to measure.
        // Previously this only wiped the pacquet revisions — if
        // `--with-pnpm` was set, pnpm's `node_modules` survived between
        // iterations, and after the warmup pnpm just hit a no-op
        // "already installed" code path instead of doing real work.
        //
        // Per-iteration cleanup paths come from the scenario: cold-cache
        // scenarios wipe `node_modules` and `store-dir`, hot-cache wipes
        // only `node_modules` so the warmup-populated store survives
        // into the timed runs.
        let cleanup_paths =
            self.scenario.expect("scenario set when benchmark() is reached").cleanup_paths();
        let cleanup_targets = self
            .revision_ids()
            .chain(self.with_pnpm.then_some(WorkEnv::PNPM))
            .map(|id| self.bench_dir(id))
            .flat_map(|dir| cleanup_paths.iter().map(move |name| dir.join(name)))
            .map(|path| path.maybe_quote().to_string())
            .join(" ");
        let cleanup_command = format!("rm -rf {cleanup_targets}");

        let mut command = Command::new("hyperfine");
        command.current_dir(self.root()).arg("--prepare").arg(&cleanup_command);

        self.hyperfine_options.append_to(&mut command);

        for id in self.revision_ids().chain(self.with_pnpm.then_some(WorkEnv::PNPM)) {
            command.arg("--command-name").arg(id.to_string()).arg(self.bash_command(id));
        }

        command
            .arg("--export-json")
            .arg(self.root().join("BENCHMARK_REPORT.json"))
            .arg("--export-markdown")
            .arg(self.root().join("BENCHMARK_REPORT.md"));

        executor("hyperfine")(&mut command);
    }

    pub fn run(&self) {
        self.init();
        self.build();
        self.benchmark();
    }
}

fn create_package_json(dst_dir: &Path, src_dir: Option<&Path>) {
    let dst = dst_dir.join("package.json");
    if let Some(src_dir) = src_dir {
        let src = src_dir.join("package.json");
        assert!(src.is_file(), "{src:?} must be a file");
        assert_ne!(src, dst);
        fs::copy(src, dst).expect("copy package.json for the revision");
    } else {
        fs::write(dst, PACKAGE_JSON).expect("write package.json for the revision");
    }
}

/// Synthesize the per-revision `pnpm-workspace.yaml` through a typed
/// [`MinimalWorkspaceManifest`] and emit it via `serde_saphyr`, instead
/// of formatting raw YAML strings. The typed round-trip rules out the
/// `duplicated mapping key` failure modes a string-injection approach
/// is prone to, and keeps the on-disk file in sync with the schema as
/// new fields are added.
///
/// Pacquet's `.npmrc` sets `ignore-scripts=true` so no scripts actually
/// run, but pnpm still warns about `ERR_PNPM_IGNORED_BUILDS` for
/// packages whose postinstalls would have fired — the manifest's
/// `allowBuilds: {core-js: false, es5-ext: false, fsevents: false}`
/// silences those specific warnings and keeps pnpm's output clean so
/// hyperfine doesn't see stderr noise.
///
/// Always guarantees `storeDir: ./store-dir` ends up in the destination.
/// Both pnpm and pacquet read the store path from this file (pacquet
/// since the `.npmrc` parser explicitly ignores `store-dir`); without
/// that key, both fall through to the global default store and the
/// benchmark's per-iteration / pre-benchmark cleanup wipes a directory
/// the install never wrote to. That silently invalidates cold/hot-cache
/// semantics and lets state from previous runs leak in (Copilot review
/// on #296).
///
/// If a custom fixture's workspace file already declares `storeDir`,
/// trust it — that's the user opting into a different store layout
/// (e.g. shared store across revisions to test a specific scenario).
/// Only inject our default when the key is absent.
///
/// Also mirrors the `.npmrc` settings (`registry`, `autoInstallPeers`,
/// `ignoreScripts`, `lockfile`) into the workspace file as camelCase
/// keys so pnpm 10 picks them up from either source. Per pnpm's reader
/// (`config/reader/src/index.ts:802-808` at pnpm/pnpm@8eb1be4988),
/// non-camelCase keys in `pnpm-workspace.yaml` are silently dropped, so
/// the npmrc spelling can't be reused verbatim. Pacquet's npmrc parser
/// is the only thing that reads `.npmrc` here; pnpm reads both.
fn create_pnpm_workspace(
    dst_dir: &Path,
    src_dir: Option<&Path>,
    registry: &str,
    scenario: BenchmarkScenario,
) {
    let dst = dst_dir.join("pnpm-workspace.yaml");
    let mut manifest = if let Some(src_dir) = src_dir {
        let src = src_dir.join("pnpm-workspace.yaml");
        if src.is_file() {
            assert_ne!(src, dst);
            let text = fs::read_to_string(&src).expect("read fixture pnpm-workspace.yaml");
            let parsed: MinimalWorkspaceManifest =
                serde_saphyr::from_str(&text).expect("parse fixture pnpm-workspace.yaml");
            if parsed.store_dir.is_none() {
                eprintln!(
                    "warn: fixture's pnpm-workspace.yaml has no top-level `storeDir:` — \
                     injecting `storeDir: ./store-dir` so per-revision store isolation works",
                );
            }
            parsed
        } else {
            MinimalWorkspaceManifest::default_for_benchmark()
        }
    } else {
        MinimalWorkspaceManifest::default_for_benchmark()
    };
    if manifest.store_dir.is_none() {
        manifest.store_dir = Some("./store-dir".to_string());
    }
    manifest.registry = Some(registry.to_string());
    manifest.auto_install_peers = Some(true);
    manifest.ignore_scripts = Some(true);
    manifest.lockfile = Some(scenario.lockfile_enabled());
    let yaml = serde_saphyr::to_string(&manifest).expect("serialize pnpm-workspace.yaml");
    fs::write(dst, yaml).expect("write pnpm-workspace.yaml for the revision");
}

fn create_npmrc(dir: &Path, registry: &str, scenario: BenchmarkScenario) {
    let path = dir.join(".npmrc");
    eprintln!("Creating config file {path:?}...");
    let mut file = File::create(path).expect("create .npmrc");
    writeln!(file, "registry={registry}").unwrap();
    // `store-dir` is read from `pnpm-workspace.yaml` (`storeDir`) by both
    // pnpm and pacquet, not from `.npmrc`. Pacquet's `.npmrc` parser
    // (`crates/npmrc/src/npmrc_auth.rs`) explicitly ignores `store-dir`
    // and a test there pins that behaviour. The static fixture's
    // `storeDir: ./store-dir` already resolves to `{bench_dir}/store-dir`
    // under each per-revision CWD, which gives the same per-revision
    // isolation the redundant `.npmrc` line was supposedly providing.
    writeln!(file, "auto-install-peers=true").unwrap();
    writeln!(file, "ignore-scripts=true").unwrap();
    writeln!(file, "{}", scenario.npmrc_lockfile_setting()).unwrap();
}

fn may_create_lockfile(dst_dir: &Path, scenario: BenchmarkScenario, src_dir: Option<&Path>) {
    let load_lockfile = || -> Cow<'_, str> {
        let Some(src_dir) = src_dir else { return Cow::Borrowed(LOCKFILE) };
        src_dir
            .join("pnpm-lock.yaml")
            .pipe(fs::read_to_string)
            .expect("read fixture lockfile")
            .pipe(Cow::Owned)
    };
    if let Some(lockfile) = scenario.lockfile(load_lockfile) {
        let path = dst_dir.join("pnpm-lock.yaml");
        fs::write(path, lockfile).expect("write pnpm-lock.yaml for the revision");
    }
}

fn create_install_script(dir: &Path, scenario: BenchmarkScenario, for_pnpm: bool) {
    let path = dir.join("install.bash");

    eprintln!("Creating script {path:?}...");
    let mut file = File::create(&path).expect("create install.bash");

    writeln!(file, "#!/bin/bash").unwrap();
    writeln!(file, "set -o errexit -o nounset -o pipefail").unwrap();
    writeln!(file, r#"cd "$(dirname "$0")""#).unwrap();

    let command = if for_pnpm { "pnpm" } else { "./pacquet/target/release/pacquet" };
    write!(file, "exec {command} install").unwrap();
    for arg in scenario.install_args() {
        write!(file, " {arg}").unwrap();
    }
    writeln!(file).unwrap();

    make_file_executable(&file).expect("make the script executable");
}

#[derive(Debug, Clone, Copy)]
enum BenchId<'a> {
    PacquetRevision(&'a str),
    Static(&'a str),
}

impl<'a> fmt::Display for BenchId<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchId::PacquetRevision(revision) => write!(f, "pacquet@{revision}"),
            BenchId::Static(name) => write!(f, "{name}"),
        }
    }
}
