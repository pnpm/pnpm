//! The pull-based watch agent: poll a git repository for new revisions of
//! a branch, materialize each one into a persistent checkout, and run the
//! pipeline against it in a child process.
//!
//! This is the smallest thing that is actually CI — no leasing, no
//! webhook ingress, no coordinator: one daemon, one repository, one
//! branch. Execution deliberately happens in a spawned `pnpm pipeline`
//! process rather than in this one: the agent is the component that runs
//! arbitrary workspace code, and its blast radius is the child process
//! and the checkout, nothing else the agent holds.

use pnpm_crypto_hash::create_short_hash;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub struct WatchInvocation {
    pub pipeline_name: Option<String>,
    /// The repository to poll — a URL or a local path, anything `git`
    /// accepts as a remote.
    pub repo: String,
    pub branch: String,
    pub interval: Duration,
    /// Poll once, build if there is a new revision, and exit.
    pub once: bool,
    pub no_cache: bool,
    pub report: bool,
    pub report_to: Option<String>,
    /// Forwarded to the child so a build can authenticate its reporting
    /// (and its installs) from an explicit auth file.
    pub npmrc_auth_file: Option<PathBuf>,
}

/// Where the agent keeps its state for one `(repo, branch)`: the
/// persistent checkout builds run in, and the last revision it built.
struct AgentDirs {
    checkout: PathBuf,
    head_file: PathBuf,
}

pub fn run_watch(invocation: &WatchInvocation, cache_dir: &Path) -> miette::Result<()> {
    let agent_dir = cache_dir
        .join("pipeline")
        .join("agent")
        .join(create_short_hash(&format!("{}\0{}", invocation.repo, invocation.branch)));
    fs::create_dir_all(&agent_dir)
        .map_err(|error| miette::miette!("creating the agent directory: {error}"))?;
    // Named after the repository so everything derived from the checkout
    // path — the run record's workspace identity above all — reads as the
    // project, not as "checkout".
    let dirs = AgentDirs {
        checkout: agent_dir.join(repo_basename(&invocation.repo)),
        head_file: agent_dir.join("head"),
    };
    println!(
        "Watching {} ({}) every {}s; checkout: {}",
        invocation.repo,
        invocation.branch,
        invocation.interval.as_secs(),
        dirs.checkout.display(),
    );
    loop {
        match poll_and_build(invocation, &dirs) {
            Ok(Some(revision)) => println!("Built {revision}."),
            Ok(None) => println!("{} is up to date.", invocation.branch),
            // A failed poll (the remote is briefly unreachable, a fetch
            // hiccup) must not kill a daemon; the next tick retries.
            Err(error) => eprintln!("[WARN] poll failed: {error}"),
        }
        if invocation.once {
            return Ok(());
        }
        std::thread::sleep(invocation.interval);
    }
}

/// One tick: compare the remote head with the last revision built, and
/// build it if they differ. Returns the revision built, or `None` when
/// there was nothing new.
fn poll_and_build(
    invocation: &WatchInvocation,
    dirs: &AgentDirs,
) -> miette::Result<Option<String>> {
    let head = remote_head(&invocation.repo, &invocation.branch)?;
    let last_built = fs::read_to_string(&dirs.head_file).ok();
    if last_built.as_deref().map(str::trim) == Some(head.as_str()) {
        return Ok(None);
    }
    materialize(invocation, dirs, &head)?;
    println!("New revision {head}; running the pipeline…");
    run_pipeline_in_checkout(invocation, dirs, &head, last_built.as_deref());
    // Recorded whether the build passed or failed: CI builds a revision
    // once and the record (submitted before the child's failure exit)
    // holds the verdict. Only a new revision triggers a new build.
    fs::write(&dirs.head_file, &head)
        .map_err(|error| miette::miette!("recording the built revision: {error}"))?;
    Ok(Some(head))
}

/// The repository's short name: the last path segment, minus a `.git`
/// suffix, reduced to the identifier alphabet. Falls back to "checkout"
/// for a remote it cannot name.
fn repo_basename(repo: &str) -> String {
    let tail = repo
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git");
    let name: String = tail
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(50)
        .collect();
    let name = name.trim_start_matches('.').to_string();
    if name.is_empty() { "checkout".to_string() } else { name }
}

fn remote_head(repo: &str, branch: &str) -> miette::Result<String> {
    let output = Command::new("git")
        .args(["ls-remote", "--", repo, &format!("refs/heads/{branch}")])
        .output()
        .map_err(|error| miette::miette!("running git ls-remote: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(miette::miette!("git ls-remote {repo} failed: {stderr}"));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| miette::miette!("branch {branch} does not exist in {repo}"))
}

/// Clone on first contact, fetch afterwards, and pin the working tree to
/// exactly `revision`. The checkout is persistent on purpose: gitignored
/// build outputs and `node_modules` survive between builds, so cache
/// restores and repeat installs stay warm.
fn materialize(
    invocation: &WatchInvocation,
    dirs: &AgentDirs,
    revision: &str,
) -> miette::Result<()> {
    if dirs.checkout.join(".git").exists() {
        git_ok(Some(&dirs.checkout), &["fetch", "origin", &invocation.branch])?;
    } else {
        git_ok(
            None,
            &[
                "clone",
                "--branch",
                &invocation.branch,
                "--",
                &invocation.repo,
                &dirs.checkout.to_string_lossy(),
            ],
        )?;
    }
    git_ok(Some(&dirs.checkout), &["checkout", "--detach", revision])
}

fn git_ok(cwd: Option<&Path>, args: &[&str]) -> miette::Result<()> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output =
        command.output().map_err(|error| miette::miette!("running git {args:?}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    Err(miette::miette!("git {args:?} failed: {stderr}"))
}

/// Run `pnpm pipeline` against the checkout as a child process. The base
/// for affected selection is the previously built revision — "what
/// changed since the last build" is the agent's native question — with
/// the first build of a checkout running the full graph. A failing build
/// is the child's verdict to report, not an agent error: the agent logs
/// it and keeps watching.
fn run_pipeline_in_checkout(
    invocation: &WatchInvocation,
    dirs: &AgentDirs,
    revision: &str,
    last_built: Option<&str>,
) {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            eprintln!("[WARN] cannot locate the pnpm executable: {error}");
            return;
        }
    };
    let mut child = Command::new(program);
    child.arg("pipeline");
    if let Some(name) = &invocation.pipeline_name {
        child.arg(name);
    }
    child.arg("--dir").arg(&dirs.checkout);
    match last_built {
        Some(last_built) => {
            child.arg("--base").arg(last_built.trim());
        }
        None => {
            child.arg("--full");
        }
    }
    if invocation.no_cache {
        child.arg("--no-cache");
    }
    if invocation.report {
        child.arg("--report");
    }
    if let Some(report_to) = &invocation.report_to {
        child.arg("--report-to").arg(report_to);
    }
    if let Some(auth_file) = &invocation.npmrc_auth_file {
        child.arg("--npmrc-auth-file").arg(auth_file);
    }
    match child.status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("[WARN] pipeline for {revision} failed with {status}");
        }
        Err(error) => eprintln!("[WARN] failed to spawn the pipeline for {revision}: {error}"),
    }
}
