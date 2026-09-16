use crate::workspaces::{Expectation, Workspace};
use cargo_lock::Lockfile;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    time::Instant,
};

pub struct Outcome {
    pub verdict: Verdict,
    pub duration_secs: f64,
    pub stage: &'static str,
    pub message: String,
    pub work_dir: PathBuf,
}

/// What a comparison means once the workspace's expectation is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The resolvers agree, as expected.
    Agree,
    /// They differ, as a tracked issue says they still do.
    KnownDifference,
    /// They differ where they should agree, or agree where a tracked issue
    /// says they should not, which means the issue is fixed and the
    /// expectation is stale.
    Unexpected,
}

impl Outcome {
    pub fn accepted(&self) -> bool {
        self.verdict != Verdict::Unexpected
    }

    pub fn label(&self) -> &'static str {
        match self.verdict {
            Verdict::Agree => "AGREE",
            Verdict::KnownDifference => "KNOWN",
            Verdict::Unexpected => "FAIL",
        }
    }
}

/// Resolve `workspace` both ways and compare, stopping at the first stage
/// that fails.
pub fn run(workspace: &Workspace, root: &Path, pnpm: &Path, cargo: &Path) -> Outcome {
    let started = Instant::now();
    let finish = |stage, message: String| {
        let (verdict, message) = judge(workspace.expectation, stage, message);
        Outcome {
            verdict,
            duration_secs: started.elapsed().as_secs_f64(),
            stage,
            message,
            work_dir: root.to_path_buf(),
        }
    };

    let cargo_dir = root.join("cargo");
    let pnpm_dir = root.join("pnpm");
    for directory in [&cargo_dir, &pnpm_dir] {
        if let Err(error) = lay_out(workspace, directory) {
            return finish("prepare", error);
        }
    }
    if let Err(error) = enable_cargo_support(&pnpm_dir) {
        return finish("prepare", error);
    }

    if let Err(error) = resolve_with_cargo(cargo, &cargo_dir) {
        return finish("cargo", error);
    }
    if let Err(error) = resolve_with_pnpm(pnpm, &pnpm_dir) {
        return finish("pnpm", error);
    }

    match compare(&cargo_dir, &pnpm_dir) {
        Err(error) => finish("compare", error),
        Ok(()) => match accepts_locked(workspace, cargo, root, &pnpm_dir) {
            Err(error) => finish("locked", error),
            Ok(()) => finish("compare", String::new()),
        },
    }
}

/// Read a comparison against what the workspace is expected to show.
///
/// Only the stages that compare the two resolvers can be a known
/// difference. A workspace that fails to lay out or to resolve at all is a
/// broken run whatever is expected of it.
fn judge(expectation: Expectation, stage: &'static str, message: String) -> (Verdict, String) {
    let comparing = matches!(stage, "compare" | "locked");
    match (expectation, message.is_empty()) {
        (Expectation::Agree, true) => (Verdict::Agree, message),
        (Expectation::Differ { issue }, true) => (
            Verdict::Unexpected,
            format!("agrees with cargo, so {issue} looks fixed; expect agreement instead"),
        ),
        (Expectation::Differ { issue }, false) if comparing => {
            (Verdict::KnownDifference, format!("{message} ({issue})"))
        }
        (_, false) => (Verdict::Unexpected, message),
    }
}

/// Write a workspace's manifests, giving every package an empty crate root.
fn lay_out(workspace: &Workspace, directory: &Path) -> Result<(), String> {
    for (path, contents) in workspace.files {
        let file = directory.join(path);
        let parent = file.parent().unwrap_or(directory);
        fs::create_dir_all(parent).map_err(|error| format!("create {parent:?}: {error}"))?;
        fs::write(&file, contents).map_err(|error| format!("write {file:?}: {error}"))?;
        if path.ends_with("Cargo.toml") && contents.contains("[package]") {
            let source = parent.join("src");
            fs::create_dir_all(&source).map_err(|error| format!("create {source:?}: {error}"))?;
            fs::write(source.join("lib.rs"), "")
                .map_err(|error| format!("write {source:?}/lib.rs: {error}"))?;
        }
    }
    Ok(())
}

fn enable_cargo_support(directory: &Path) -> Result<(), String> {
    let path = directory.join("pnpm-workspace.yaml");
    fs::write(&path, "packages: []\ncargo:\n  enabled: true\n")
        .map_err(|error| format!("write {path:?}: {error}"))
}

fn resolve_with_cargo(cargo: &Path, directory: &Path) -> Result<(), String> {
    run_command(Command::new(cargo).current_dir(directory).arg("generate-lockfile"))
}

fn resolve_with_pnpm(pnpm: &Path, directory: &Path) -> Result<(), String> {
    run_command(
        Command::new(pnpm)
            .current_dir(directory)
            .env("PNPM_CONFIG_CACHE_DIR", directory.join(".cache"))
            .env("PNPM_CONFIG_STORE_DIR", directory.join(".store"))
            .args(["install", "--lockfile-only", "--no-frozen-lockfile"]),
    )
}

/// Whether `cargo` accepts pnpm's lockfile as already resolved.
///
/// The lockfile is read in a copy of the workspace that pnpm never touched,
/// so `--locked` measures the lockfile alone rather than the source
/// replacement pnpm writes alongside it.
fn accepts_locked(
    workspace: &Workspace,
    cargo: &Path,
    root: &Path,
    pnpm_dir: &Path,
) -> Result<(), String> {
    let directory = root.join("locked");
    lay_out(workspace, &directory)?;
    let lockfile = pnpm_dir.join("Cargo.lock");
    fs::copy(&lockfile, directory.join("Cargo.lock"))
        .map_err(|error| format!("copy {lockfile:?}: {error}"))?;
    run_command(
        Command::new(cargo)
            .current_dir(&directory)
            .args(["metadata", "--locked", "--format-version", "1"]),
    )
}

/// Compare what each resolver locked, crate by crate.
fn compare(cargo_dir: &Path, pnpm_dir: &Path) -> Result<(), String> {
    let expected = locked_crates(cargo_dir)?;
    let received = locked_crates(pnpm_dir)?;
    let differences = expected
        .iter()
        .filter(|crate_| !received.contains(*crate_))
        .map(|crate_| format!("cargo locked {crate_}, pnpm did not"))
        .chain(
            received
                .iter()
                .filter(|crate_| !expected.contains(*crate_))
                .map(|crate_| format!("pnpm locked {crate_}, cargo did not")),
        )
        .collect::<Vec<_>>();
    if differences.is_empty() {
        return Ok(());
    }
    Err(differences.join("; "))
}

fn locked_crates(directory: &Path) -> Result<Vec<String>, String> {
    let path = directory.join("Cargo.lock");
    let contents = fs::read_to_string(&path).map_err(|error| format!("read {path:?}: {error}"))?;
    let lockfile =
        Lockfile::from_str(&contents).map_err(|error| format!("parse {path:?}: {error}"))?;
    let mut crates = lockfile.packages
        .iter()
        .map(|package| format!("{} {}", package.name, package.version))
        .collect::<Vec<_>>();
    crates.sort();
    Ok(crates)
}

fn run_command(command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("run {:?}: {error}", command.get_program()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{:?} failed: {}", command.get_program(), stderr.trim()))
}
