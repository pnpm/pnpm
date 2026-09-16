//! The interpreters this machine offers, and how pnpm starts one.
//!
//! pnpm runs an interpreter by the path it located, not by the name it
//! looked for: a name is resolved through a search path the workspace
//! being installed is not on, since an install started from a script has
//! the workspace's own `bin` directories on its PATH, where a dependency
//! can leave an executable named like an interpreter.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tokio::process::Command;

/// How to run one interpreter: the program, and any arguments of its own
/// that come before the ones the host helper needs.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct InterpreterCommand {
    pub(super) program: String,
    pub(super) arguments: Vec<String>,
}

impl InterpreterCommand {
    pub(super) fn program(program: impl Into<String>) -> Self {
        Self { program: program.into(), arguments: Vec::new() }
    }

    /// The same interpreter, run by the path its name resolves to on the
    /// search path. `None` when the name is not on it.
    pub(super) fn located(self, path: &OsString) -> Option<Self> {
        Some(Self { program: locate(&self.program, path)?, ..self })
    }

    fn at(self, program: &str) -> Self {
        Self { program: program.to_string(), ..self }
    }

    /// The Windows launcher, which selects an installed version by
    /// argument rather than by executable name.
    pub(super) fn launcher(argument: String) -> Self {
        Self { program: "py".to_string(), arguments: vec![argument] }
    }
}

impl std::fmt::Display for InterpreterCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.program)?;
        for argument in &self.arguments {
            write!(formatter, " {argument}")?;
        }
        Ok(())
    }
}

/// Every interpreter this machine offers under a version, newest first:
/// what it has beyond the conventional names.
pub(super) async fn scan_for_interpreters(path: &OsString) -> Vec<InterpreterCommand> {
    let mut found = versioned_interpreters_on_path(path);
    if cfg!(windows) {
        found.extend(launcher_interpreters(path).await);
    }
    found
}

/// This process's PATH, without the directories inside the workspace
/// being installed.
///
/// An install started from a script has the workspace's own `bin`
/// directories on its PATH, where a dependency can leave an executable
/// named like an interpreter. pnpm resolves no interpreter through them:
/// installing a project must not run what the project's dependencies
/// ship.
pub(super) fn path_outside(workspace: Option<&Path>) -> OsString {
    let path = env::var_os("PATH").unwrap_or_default();
    let Some(workspace) = workspace else { return path };
    // A PATH entry can name the workspace the way it was configured or the
    // way the filesystem resolves it, and a workspace that cannot be
    // resolved at all still has the name it was configured under. Nothing
    // that names it either way is searched.
    let spellings: Vec<PathBuf> =
        [Some(workspace.to_path_buf()), dunce::canonicalize(workspace).ok()]
            .into_iter()
            .flatten()
            .collect();
    let inside = |directory: &Path| {
        spellings
            .iter()
            .any(|workspace| directory.starts_with(workspace))
    };
    let outside = env::split_paths(&path)
        .filter(|directory| {
            // An empty entry names the directory pnpm runs in, which for an
            // install is the workspace itself.
            !directory.as_os_str().is_empty()
                && !inside(directory)
                && !dunce::canonicalize(directory).is_ok_and(|directory| inside(&directory))
        });
    env::join_paths(outside).expect("a PATH this process was given joins back together")
}

/// Each `python3.<minor>` on the PATH by the path it resolves to, so
/// that a minor version installed in two directories is two candidates
/// rather than the one the PATH happens to resolve, and so that what was
/// checked is what runs.
pub(super) fn versioned_interpreters_on_path(path: &OsString) -> Vec<InterpreterCommand> {
    let mut by_minor: BTreeMap<Reverse<u64>, Vec<InterpreterCommand>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for directory in env::split_paths(path) {
        for (minor, interpreter) in interpreters_in(&directory) {
            // One interpreter reached through several directories is one
            // interpreter; two of the same minor version are two.
            if seen.insert(interpreter.clone()) {
                by_minor
                    .entry(Reverse(minor))
                    .or_default()
                    .push(InterpreterCommand::program(interpreter));
            }
        }
    }
    by_minor
        .into_values()
        .flatten()
        .collect()
}

/// The interpreters one directory holds, by the path each resolves to. A
/// name that resolves to nothing is no interpreter.
pub(super) fn interpreters_in(directory: &Path) -> Vec<(u64, String)> {
    let Ok(entries) = fs::read_dir(directory) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let minor = interpreter_minor(&entry.file_name())?;
            let interpreter = dunce::canonicalize(entry.path()).ok()?;
            Some((minor, interpreter.to_str()?.to_string()))
        })
        .collect()
}

pub(super) fn interpreter_minor(name: &std::ffi::OsStr) -> Option<u64> {
    let name = name.to_str()?;
    let name = if cfg!(windows) { name.strip_suffix(".exe").unwrap_or(name) } else { name };
    name.strip_prefix("python3.")?
        .parse()
        .ok()
}

/// The interpreters the Windows launcher can run, by the selectors it
/// prints for them, which name third-party runtimes as well as versions.
/// A launcher too old to list them still runs its own default.
pub(super) async fn launcher_interpreters(path: &OsString) -> Vec<InterpreterCommand> {
    let Some(launcher) = locate("py", path) else { return Vec::new() };
    let Ok(listed) = Command::new(&launcher)
        .arg("--list")
        .env("PATH", path)
        .output()
        .await
    else {
        return Vec::new();
    };
    let selectors: Vec<InterpreterCommand> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| {
            let selector = line.split_whitespace().next()?;
            selector
                .starts_with('-')
                .then(|| InterpreterCommand::launcher(selector.to_string()).at(&launcher))
        })
        .collect();
    if selectors.is_empty() {
        vec![InterpreterCommand::launcher("-3".to_string()).at(&launcher)]
    } else {
        selectors
    }
}

/// The program a name resolves to on the search path, as the path to
/// run: pnpm starts an interpreter it located rather than a name, which
/// Windows would also look for beside the process and in the directory it
/// runs from.
pub(super) fn locate(name: &str, path: &OsString) -> Option<String> {
    let located = which::which_in_global(name, Some(path)).ok()?.next()?;
    dunce::canonicalize(located)
        .ok()?
        .to_str()
        .map(str::to_string)
}
