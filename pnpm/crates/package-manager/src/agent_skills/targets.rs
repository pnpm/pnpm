use super::recorded_path;
use pnpm_config::Config;
use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

/// Environment variables by which an agent identifies itself, and the
/// agent skill directory each one reads.
const AGENT_ENV_DIRS: &[(&str, &str)] = &[
    ("CLAUDECODE", ".claude/skills"),
    ("CURSOR_AGENT", ".cursor/skills"),
    ("GEMINI_CLI", ".gemini/skills"),
];

/// The agent skill directory of the agent running pnpm, when one
/// identifies itself in the environment.
#[must_use]
pub fn agent_skills_dir_from_env() -> Option<&'static str> {
    AGENT_ENV_DIRS
        .iter()
        .find(|(var, _)| std::env::var_os(var).is_some_and(|value| !value.is_empty()))
        .map(|(_, dir)| *dir)
}

/// The directories to link skills into, created when missing.
///
/// `skills.dirs` replaces detection. Otherwise these are the `.*/skills`
/// directories that already exist at the workspace root, plus the one of
/// `agent_dir`. Two paths to one directory count once.
pub(super) fn target_dirs(
    config: &Config,
    workspace_root: &Path,
    agent_dir: Option<&str>,
) -> Result<Vec<PathBuf>, (PathBuf, io::Error)> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for dir in candidate_dirs(config, workspace_root, agent_dir) {
        fs::create_dir_all(&dir).map_err(|error| (dir.clone(), error))?;
        let canonical = fs::canonicalize(&dir).map_err(|error| (dir.clone(), error))?;
        if seen.insert(canonical) {
            targets.push(dir);
        }
    }
    Ok(targets)
}

/// Whether the next sync would link into other directories than the ones
/// `linked` is in. Every target receives every entry, so the parents of
/// `linked` are the previous targets. A missing candidate counts as a change
/// because the sync creates it.
pub(super) fn targets_changed(
    config: &Config,
    workspace_root: &Path,
    agent_dir: Option<&str>,
    linked: &[String],
) -> bool {
    let previous: BTreeSet<String> = linked
        .iter()
        .filter_map(|entry| Path::new(entry).parent())
        .map(|dir| dir.to_string_lossy().replace('\\', "/"))
        .collect();
    let mut seen = HashSet::new();
    let mut current = BTreeSet::new();
    for dir in candidate_dirs(config, workspace_root, agent_dir) {
        let Ok(canonical) = fs::canonicalize(&dir) else { return true };
        if seen.insert(canonical) {
            current.insert(recorded_path(workspace_root, &dir));
        }
    }
    previous != current
}

fn candidate_dirs(config: &Config, workspace_root: &Path, agent_dir: Option<&str>) -> Vec<PathBuf> {
    if let Some(dirs) = &config.skills_dirs {
        return dirs
            .iter()
            .map(|dir| workspace_root.join(dir))
            .collect();
    }
    let mut dirs = existing_agent_dirs(workspace_root);
    dirs.extend(agent_dir.map(|dir| workspace_root.join(dir)));
    dirs
}

/// `<workspace_root>/.<agent>/skills` for every agent directory present.
fn existing_agent_dirs(workspace_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(workspace_root) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        })
        .map(|entry| entry.path().join("skills"))
        .filter(|dir| dir.is_dir())
        .collect();
    dirs.sort();
    dirs
}
