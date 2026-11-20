use crate::workspaces::{
    Expectation,
    Workspace,
};
use cargo_lock::Lockfile;
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
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
    let finish = |stage, finding| {
        let (verdict, message) = judge(workspace.expectation, finding);
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
            return finish("prepare", Finding::Broken(error));
        }
    }
    if let Err(error) = enable_cargo_support(&pnpm_dir) {
        return finish("prepare", Finding::Broken(error));
    }

    if let Err(error) = resolve_with_cargo(cargo, &cargo_dir) {
        return finish("cargo", Finding::Broken(error));
    }
    if let Err(error) = resolve_with_pnpm(pnpm, &pnpm_dir) {
        return finish("pnpm", Finding::Broken(error));
    }

    match compare(&cargo_dir, &pnpm_dir) {
        Err(broken) => finish("compare", Finding::Broken(broken)),
        Ok(Finding::Agree) => match accepts_locked(workspace, cargo, root, &pnpm_dir) {
            Err(broken) => finish("locked", Finding::Broken(broken)),
            Ok(finding) => finish("locked", finding),
        },
        Ok(finding) => finish("compare", finding),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    /// The resolvers produced the same thing.
    Agree,
    /// The resolvers produced different things, which is the only finding a
    /// known gap is allowed to be.
    Differ(String),
    /// The stage could not reach a conclusion: a workspace that would not
    /// lay out, a resolver that would not run, a lockfile that would not
    /// parse. Never a known gap, whatever is expected of the workspace.
    Broken(String),
}

/// Read a finding against what the workspace is expected to show.
fn judge(expectation: Expectation, finding: Finding) -> (Verdict, String) {
    match (expectation, finding) {
        (_, Finding::Broken(message)) => (Verdict::Unexpected, message),
        (Expectation::Agree, Finding::Agree) => (Verdict::Agree, String::new()),
        (Expectation::Agree, Finding::Differ(message)) => (Verdict::Unexpected, message),
        (Expectation::Differ { issue }, Finding::Agree) => (
            Verdict::Unexpected,
            format!("agrees with cargo, so {issue} looks fixed; expect agreement instead"),
        ),
        (Expectation::Differ { issue }, Finding::Differ(message)) => {
            (Verdict::KnownDifference, format!("{message} ({issue})"))
        }
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
) -> Result<Finding, String> {
    let directory = root.join("locked");
    lay_out(workspace, &directory)?;
    let lockfile = pnpm_dir.join("Cargo.lock");
    fs::copy(&lockfile, directory.join("Cargo.lock"))
        .map_err(|error| format!("copy {lockfile:?}: {error}"))?;
    let refused = command_outcome(
        Command::new(cargo)
            .current_dir(&directory)
            .args(["metadata", "--locked", "--format-version", "1"]),
    )?;
    Ok(match refused {
        None => Finding::Agree,
        Some(stderr) => Finding::Differ(format!("cargo rejected pnpm's lockfile: {stderr}")),
    })
}

fn compare(cargo_dir: &Path, pnpm_dir: &Path) -> Result<Finding, String> {
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
    Ok(if differences.is_empty() {
        Finding::Agree
    } else {
        Finding::Differ(differences.join("; "))
    })
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
    match command_outcome(command)? {
        None => Ok(()),
        Some(stderr) => Err(format!("{:?} failed: {stderr}", command.get_program())),
    }
}

/// Run a command, telling "it would not start" from "it said no": the
/// first is a broken run, the second can be a finding about the lockfile.
fn command_outcome(command: &mut Command) -> Result<Option<String>, String> {
    let output = command
        .output()
        .map_err(|error| format!("run {:?}: {error}", command.get_program()))?;
    if output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stderr).trim().to_string()))
}

#[cfg(test)]
mod tests;
