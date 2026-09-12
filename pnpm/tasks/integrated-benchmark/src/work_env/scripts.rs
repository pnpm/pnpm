use super::{BENCHMARK_OUTPUT_LOG, BenchId, PREWARM_SCRIPT};
use crate::{
    cli_args::{BenchmarkScenario, Cleanup},
    fixtures::LOCKFILE,
    verify::executor,
};
use itertools::Itertools;
use os_display::Quotable;
use pipe_trait::Pipe;
use pnpm_fs::file_mode::make_file_executable;
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

/// Empty one benchmark directory's install state and metrics log.
pub(super) fn wipe_bench_dir(dir: &Path) {
    for name in ["node_modules", "store-dir", "cache-dir", "pnpr-storage", "cold-mock-storage"] {
        let path = dir.join(name);
        if path.exists() {
            remove_dir_all_with_retry(&path).expect("pre-benchmark wipe");
        }
    }
    let output_log = dir.join(BENCHMARK_OUTPUT_LOG);
    if output_log.exists() {
        fs::remove_file(output_log).expect("pre-benchmark metrics-log wipe");
    }
}
/// Whether `dir` contains at least one regular file, recursively. Used to
/// confirm a pnpr server actually wrote something (i.e. served a resolve).
/// A missing/unreadable dir counts as empty.
pub(super) fn dir_contains_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            return true;
        }
        if path.is_dir() && dir_contains_file(&path) {
            return true;
        }
    }
    false
}
/// Fetch `commit` into `revision_repo`, creating it if missing, and
/// check the commit out. Shared between the pacquet and pnpm build
/// paths — both follow the same fetch-by-SHA discipline that PR [#321]
/// established for pacquet revisions.
///
/// [#321]: https://github.com/pnpm/pacquet/pull/321
pub(super) fn sync_bench_repo(repository: &Path, revision_repo: &Path, commit: &str) {
    let had_existing_git = prepare_bench_repo(repository, revision_repo, commit);

    if had_existing_git {
        // `pnpm install` and `pnpm run compile-only` from a previous orchestrator
        // run can leave tracked files dirty (e.g. `pnpm-lock.yaml` rewritten,
        // generated `dist/*`). A fresh `git checkout <commit>` against a dirty
        // worktree fails with "Your local changes would be overwritten" — wipe
        // them first.
        eprintln!("Resetting worktree at {revision_repo:?}...");
        Command::new("git")
            .current_dir(revision_repo)
            .arg("reset")
            .arg("--hard")
            .pipe(executor("git reset --hard"));
    }

    eprintln!("Checking out {commit:?}...");
    Command::new("git")
        .current_dir(revision_repo)
        .arg("checkout")
        .arg(commit)
        .pipe(executor("git checkout"));

    eprintln!("List of branches:");
    Command::new("git").current_dir(revision_repo).arg("branch").pipe(executor("git branch"));
}
/// Prepare the clone and fetch the commit. Reports whether HEAD already
/// existed, so the caller can reset tracked build outputs before checkout.
pub(super) fn prepare_bench_repo(repository: &Path, revision_repo: &Path, commit: &str) -> bool {
    let had_existing_git = revision_repo.exists() && revision_repo.join(".git").exists();
    if revision_repo.exists() {
        if !had_existing_git {
            eprintln!("Initializing a git repository at {revision_repo:?}...");
            Command::new("git")
                .current_dir(revision_repo)
                .arg("init")
                .arg(revision_repo)
                .arg("--initial-branch=__blank__")
                .pipe(executor("git init"));
        }

        eprintln!("Fetching {commit} from {repository:?}...");
        Command::new("git")
            .current_dir(revision_repo)
            .arg("fetch")
            .arg(repository)
            .arg(commit)
            .pipe(executor("git fetch"));
    } else {
        eprintln!("Cloning {repository:?} to {revision_repo:?}...");
        Command::new("git")
            .arg("clone")
            .arg("--no-checkout")
            .arg(repository)
            .arg(revision_repo)
            .pipe(executor("git clone"));
    }

    had_existing_git
}
/// `fs::remove_dir_all` that tolerates the transient "Directory not empty"
/// error macOS/APFS raises while a just-finished install's store writes
/// settle, retrying only that kind and failing fast on any other.
pub(super) fn remove_dir_all_with_retry(path: &Path) -> std::io::Result<()> {
    const MAX_ATTEMPTS: u32 = 6;
    for attempt in 0..MAX_ATTEMPTS {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(_) if !path.exists() => return Ok(()),
            // Only the transient "Directory not empty" that APFS raises while a
            // just-finished install's store writes settle is worth retrying;
            // fail fast on anything else (permissions, I/O) instead of sleeping
            // ~4s first.
            Err(err) if err.kind() != ErrorKind::DirectoryNotEmpty => return Err(err),
            Err(err) if attempt == MAX_ATTEMPTS - 1 => return Err(err),
            Err(_) => thread::sleep(Duration::from_millis(200 * u64::from(attempt + 1))),
        }
    }
    unreachable!("the final attempt returns instead of looping")
}
/// Build the `--prepare` shell command for hyperfine: wipe every bench dir's
/// removal paths, then `cp` each pristine file that needs restoring. The
/// pieces are joined with `&&` so any failure aborts the iteration.
pub(super) fn build_cleanup_command<'a, Ids, BenchDir>(
    cleanup: &Cleanup,
    ids: Ids,
    mut bench_dir: BenchDir,
) -> String
where
    Ids: Iterator<Item = BenchId<'a>>,
    BenchDir: FnMut(BenchId<'a>) -> PathBuf,
{
    let dirs: Vec<PathBuf> = ids.map(&mut bench_dir).collect();
    let mut parts: Vec<String> = Vec::new();

    let remove_targets = dirs
        .iter()
        .flat_map(|dir| cleanup.remove.iter().map(move |name| dir.join(name)))
        .map(|path| path.maybe_quote().to_string())
        .join(" ");
    if !remove_targets.is_empty() {
        // List each target once and retry per-path: `rm -rf` on a just-emptied
        // store occasionally hits a transient APFS "Directory not empty", so
        // two short retries cover the settle window; `|| exit 1` still fails
        // the `--prepare` step (and the benchmark) if a path genuinely can't be
        // removed. Looping avoids repeating the whole (potentially long) target
        // list three times in the command string.
        parts.push(format!(
            r#"for p in {remove_targets}; do rm -rf "$p" || (sleep 0.5; rm -rf "$p") || (sleep 1; rm -rf "$p") || exit 1; done"#,
        ));
    }

    for dir in &dirs {
        for (dst, src) in cleanup.restore {
            let src_path = dir.join(src).maybe_quote().to_string();
            let dst_path = dir.join(dst).maybe_quote().to_string();
            parts.push(format!("cp {src_path} {dst_path}"));
        }
    }

    parts.join(" && ")
}
pub(super) fn may_create_lockfile(
    dst_dir: &Path,
    scenario: BenchmarkScenario,
    src_dir: Option<&Path>,
) {
    let load_lockfile = || -> Cow<'_, str> {
        let Some(src_dir) = src_dir else { return Cow::Borrowed(LOCKFILE) };
        src_dir
            .join("pnpm-lock.yaml")
            .pipe(fs::read_to_string)
            .expect("read fixture lockfile")
            .pipe(Cow::Owned)
    };
    if let Some(lockfile) = scenario.lockfile(load_lockfile) {
        // Land the seeded lockfile in a strictly later millisecond than
        // the just-written `package.json`. The kernel's file-timestamp
        // clock ticks coarsely (~1 ms), so back-to-back writes can share
        // one mtime — and the repeat-install fast path reads a manifest
        // whose mtime equals its baseline (the lockfile mtime, truncated
        // to ms) as possibly-modified, pushing every timed iteration
        // onto the heavier content-check path for one bench dir but not
        // the other. Real projects never hit this shape (the lockfile is
        // written by an install that started after the manifest edit),
        // so the pause keeps the measured runs on the representative
        // pure-mtime path.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let path = dst_dir.join("pnpm-lock.yaml");
        fs::write(path, lockfile).expect("write pnpm-lock.yaml for the revision");
    }
}
/// Write `install.bash` that invokes `command` (the resolved binary,
/// e.g. `./pacquet/target/release/pnpm` or `node .../pnpm.mjs`)
/// with the scenario's install arguments.
///
/// When `needs_pnpr_env` is set, the script sources `.pnpr-env` (written
/// at benchmark time once the per-target pnpr server has a port) so the
/// client picks up `PNPM_CONFIG_PNPR_SERVER` and routes the install
/// through it. The `source` fails loudly under `errexit` if the file is
/// missing, rather than silently falling back to a direct install.
pub(super) fn create_install_script(
    dir: &Path,
    scenario: BenchmarkScenario,
    command: &str,
    id: BenchId,
) {
    // The proxy-cache populator must reach the registry, so a scenario
    // whose measured args are offline hands it the online pre-warm args.
    let args = if id.is_proxy_cache_populator() {
        scenario.prewarm_install_args().unwrap_or_else(|| scenario.install_args())
    } else {
        scenario.install_args()
    };
    write_bench_script(dir, "install.bash", command, id, args, id.is_pacquet_like());
    if let Some(prewarm_args) = scenario.prewarm_install_args() {
        // Untimed online priming run (see
        // `BenchmarkScenario::prewarm_install_args`). Metrics capture is
        // off so the priming run can't pollute the diagnostics log the
        // timed runs append to.
        write_bench_script(dir, PREWARM_SCRIPT, command, id, prewarm_args, false);
    }
}
pub(super) fn write_bench_script(
    dir: &Path,
    file_name: &str,
    command: &str,
    id: BenchId,
    args: &[&str],
    capture_pacquet_metrics: bool,
) {
    let path = dir.join(file_name);

    eprintln!("Creating script {path:?}...");
    let mut file =
        File::create(&path).unwrap_or_else(|error| panic!("create {file_name}: {error}"));

    writeln!(file, "#!/bin/bash").unwrap();
    writeln!(file, "set -o errexit -o nounset -o pipefail").unwrap();
    writeln!(file, r#"cd "$(dirname "$0")""#).unwrap();
    if id.is_pnpr() {
        writeln!(file, "source ./.pnpr-env").unwrap();
    }
    if capture_pacquet_metrics {
        // pnpm targets cannot emit pacquet phase events, so diagnostics are
        // pacquet/pnpr-only. This adds a small one-sided tracing + file-I/O
        // cost to pnpm comparisons, but keeps materialization regressions
        // visible in the benchmark report.
        writeln!(file, r#"export TRACE="${{TRACE:-pacquet::install::phase=info}}""#).unwrap();
        writeln!(file, r#"export TRACE_FORMAT="${{TRACE_FORMAT:-json}}""#).unwrap();
        writeln!(
            file,
            r#"printf '{{"benchmarkTarget":"{id}","event":"runStart"}}\n' >> {BENCHMARK_OUTPUT_LOG}"#,
        )
        .unwrap();
    }

    write!(file, "exec {command}").unwrap();
    if capture_pacquet_metrics {
        write!(file, " --reporter ndjson").unwrap();
    }
    for arg in args {
        write!(file, " {arg}").unwrap();
    }
    if capture_pacquet_metrics {
        write!(file, " >> {BENCHMARK_OUTPUT_LOG} 2>&1").unwrap();
    }
    writeln!(file).unwrap();

    make_file_executable(&file).expect("make the script executable");
}
