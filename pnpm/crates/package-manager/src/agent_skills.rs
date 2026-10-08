//! Agent skills shipped by direct dependencies.
//!
//! A skill is a directory `skills/<name>/` holding a `SKILL.md`. pnpm never
//! reads a skill: it finds the directories, records the packages whose
//! skills await approval, and links the approved ones into the project's
//! agent skill directories as `pnpm-<package>-<skill>`.

pub(crate) use installed::resync_agent_skills_at;
pub use installed::{ResyncAgentSkillsError, resync_installed_agent_skills};
pub use targets::agent_skills_dir_from_env;

mod discovery;
mod installed;
mod targets;

use crate::{AllowBuildPolicy, VersionPolicyError};
use derive_more::{Display, Error};
use discovery::{SkillSource, discover_skill_sources};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

/// The prefix of every entry pnpm links into an agent skill directory.
pub const AGENT_SKILL_LINK_PREFIX: &str = "pnpm-";

/// What [`sync_agent_skills`] reconciles.
pub struct SyncAgentSkills<'a> {
    pub config: &'a Config,
    pub workspace_root: &'a Path,
    /// The lockfile of what is installed. Its importers name the direct
    /// dependencies.
    pub lockfile: &'a Lockfile,
    pub included: IncludedDependencies,
    /// The entries a previous sync linked, as recorded in `.modules.yaml`.
    pub linked: &'a [String],
    /// The agent skill directory of the agent running pnpm, from
    /// [`agent_skills_dir_from_env`].
    pub agent_dir: Option<&'a str>,
}

/// The agent skills state to record in `.modules.yaml`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AgentSkillsState {
    /// The depPaths of the direct dependencies whose skills await
    /// approval, sorted.
    pub pending: Vec<String>,
    /// The linked entries, relative to the workspace root, sorted.
    pub linked: Vec<String>,
}

impl AgentSkillsState {
    /// The keys `permissions` approves the pending packages under, sorted.
    #[must_use]
    pub fn pending_keys(&self) -> Vec<String> {
        self.pending
            .iter()
            .map(|dep_path| crate::allow_build_key_from_ignored_build(dep_path))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Tell the user which packages' skills await approval.
pub(crate) fn report_pending_skills<Reporter: pnpm_reporter::Reporter>(package_names: Vec<String>) {
    if package_names.is_empty() {
        return;
    }
    Reporter::emit(&pnpm_reporter::LogEvent::PendingSkills(pnpm_reporter::PendingSkillsLog {
        level: pnpm_reporter::LogLevel::Debug,
        package_names,
    }));
}

/// Errors from [`sync_agent_skills`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AgentSkillsError {
    #[display("There is no agent skill directory to link the approved skills of {packages} into")]
    #[diagnostic(
        code(ERR_PNPM_NO_AGENT_SKILLS_DIR),
        help(
            "Name the directories in pnpm-workspace.yaml, for example:\nskills:\n  dirs:\n    - .claude/skills"
        )
    )]
    NoTargetDir {
        #[error(not(source))]
        packages: String,
    },

    #[display("The agent skills of {first} and {second} would both be linked as {name}")]
    #[diagnostic(code(ERR_PNPM_AGENT_SKILL_NAME_COLLISION))]
    NameCollision { name: String, first: String, second: String },

    #[display(
        "Cannot link an agent skill at {path:?}, because pnpm did not create the entry already there"
    )]
    #[diagnostic(
        code(ERR_PNPM_AGENT_SKILL_PATH_OCCUPIED),
        help(
            "Remove the entry, or name other agent skill directories in the skills.dirs setting."
        )
    )]
    Occupied {
        #[error(not(source))]
        path: PathBuf,
    },

    #[display("Failed to update the agent skill entry at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_AGENT_SKILL_LINK))]
    Io { path: PathBuf, source: io::Error },

    #[diagnostic(transparent)]
    Policy(#[error(source)] VersionPolicyError),
}

/// Link the approved skills of the direct dependencies, prune the entries
/// that are no longer approved or targeted, and report the packages whose
/// skills await approval.
pub fn sync_agent_skills(
    input: &SyncAgentSkills<'_>,
) -> Result<AgentSkillsState, AgentSkillsError> {
    if input.config.skills_dirs.as_ref().is_some_and(Vec::is_empty) {
        prune(input.workspace_root, input.linked, &BTreeSet::new())?;
        return Ok(AgentSkillsState::default());
    }
    let sources = discover_skill_sources(input);
    let policy = AllowBuildPolicy::from_approvals(&input.config.allow_skills, false)
        .map_err(AgentSkillsError::Policy)?;
    let mut pending = Vec::new();
    let mut approved = Vec::new();
    for source in sources {
        match policy.check(&source.dep_path) {
            Some(true) => approved.push(source),
            Some(false) => {}
            None => pending.push(source.dep_path),
        }
    }
    let entries = link_entries(&approved)?;
    let dirs = if entries.is_empty() {
        Vec::new()
    } else {
        targets::target_dirs(input.config, input.workspace_root, input.agent_dir)
            .map_err(|(path, source)| AgentSkillsError::Io { path, source })?
    };
    if !entries.is_empty() && dirs.is_empty() {
        return Err(AgentSkillsError::NoTargetDir { packages: package_list(&approved) });
    }
    let desired = desired_entries(input.workspace_root, &dirs, &entries);
    prune(input.workspace_root, input.linked, &desired.keys().cloned().collect())?;
    materialize(input.workspace_root, input.linked, &desired)?;
    for dir in &dirs {
        ignore_links_in(dir)?;
    }
    pending.sort();
    Ok(AgentSkillsState { pending, linked: desired.into_keys().collect() })
}

fn package_list(sources: &[SkillSource]) -> String {
    sources
        .iter()
        .map(|source| source.approval_key.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ")
}

/// The entry name of every approved skill, mapped to the skill directory
/// it links to.
fn link_entries(approved: &[SkillSource]) -> Result<BTreeMap<String, PathBuf>, AgentSkillsError> {
    let mut entries: BTreeMap<String, (PathBuf, &str)> = BTreeMap::new();
    for source in approved {
        for skill in &source.skills {
            let name = format!("{AGENT_SKILL_LINK_PREFIX}{}-{skill}", source.link_segment);
            let target = source.package_dir.join("skills").join(skill);
            if let Some((_, first)) = entries.get(&name) {
                return Err(AgentSkillsError::NameCollision {
                    name,
                    first: (*first).to_string(),
                    second: source.dep_path.clone(),
                });
            }
            entries.insert(name, (target, &source.dep_path));
        }
    }
    Ok(entries
        .into_iter()
        .map(|(name, (target, _))| (name, target))
        .collect())
}

/// Every entry to link, keyed by the form recorded in `.modules.yaml`.
fn desired_entries(
    workspace_root: &Path,
    dirs: &[PathBuf],
    entries: &BTreeMap<String, PathBuf>,
) -> BTreeMap<String, PathBuf> {
    dirs.iter()
        .flat_map(|dir| {
            entries
                .iter()
                .map(move |(name, target)| {
                    (recorded_path(workspace_root, &dir.join(name)), target.clone())
                })
        })
        .collect()
}

fn recorded_path(workspace_root: &Path, path: &Path) -> String {
    let path = path.strip_prefix(workspace_root).unwrap_or(path);
    path.to_string_lossy().replace('\\', "/")
}

fn prune(
    workspace_root: &Path,
    linked: &[String],
    keep: &BTreeSet<String>,
) -> Result<(), AgentSkillsError> {
    for entry in linked
        .iter()
        .filter(|entry| !keep.contains(*entry))
    {
        let path = workspace_root.join(entry);
        remove_entry(&path).map_err(|source| AgentSkillsError::Io { path, source })?;
    }
    Ok(())
}

/// Remove a link, junction, or copied directory without following it.
fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) => {
            if pnpm_fs::is_symlink_or_junction(path)? {
                pnpm_fs::remove_symlink_dir(path)
            } else if metadata.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        }
    }
}

fn materialize(
    workspace_root: &Path,
    linked: &[String],
    desired: &BTreeMap<String, PathBuf>,
) -> Result<(), AgentSkillsError> {
    for (entry, target) in desired {
        let path = workspace_root.join(entry);
        let owned = linked.contains(entry) || links_to(&path, target);
        if !owned && fs::symlink_metadata(&path).is_ok() {
            return Err(AgentSkillsError::Occupied { path });
        }
        pnpm_fs::force_symlink_dir(target, &path)
            .map_err(|source| AgentSkillsError::Io { path: path.clone(), source })?;
    }
    Ok(())
}

/// Whether `path` is a link that resolves to `target`, as one a previous
/// sync created before it failed to record it.
fn links_to(path: &Path, target: &Path) -> bool {
    pnpm_fs::is_symlink_or_junction(path).unwrap_or(false)
        && matches!(
            (fs::canonicalize(path), fs::canonicalize(target)),
            (Ok(resolved), Ok(target)) if resolved == target,
        )
}

/// The ignore rule that keeps pnpm's entries out of commits: they point
/// into `node_modules`, so they would dangle in a fresh clone.
const GITIGNORE_RULE: &str = "pnpm-*";

/// Add [`GITIGNORE_RULE`] to the `.gitignore` of `dir`, keeping what the
/// file already says.
fn ignore_links_in(dir: &Path) -> Result<(), AgentSkillsError> {
    let path = dir.join(".gitignore");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(AgentSkillsError::Io { path, source }),
    };
    if existing
        .lines()
        .any(|line| line.trim() == GITIGNORE_RULE)
    {
        return Ok(());
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    fs::write(&path, format!("{existing}{separator}{GITIGNORE_RULE}\n"))
        .map_err(|source| AgentSkillsError::Io { path, source })
}

#[cfg(test)]
mod tests;
