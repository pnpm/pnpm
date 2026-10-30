//! Agent skills shipped by direct dependencies.
//!
//! A skill is a directory `skills/<name>/` holding a `SKILL.md`. pnpm never
//! reads a skill: it finds the directories, records the packages whose
//! skills await approval, and links the approved ones into the project's
//! agent skill directories under the skill's own name, as the Agent Skills
//! specification requires a skill's directory to be named after it.

pub(crate) use installed::resync_agent_skills_at;
pub use installed::{ResyncAgentSkillsError, resync_installed_agent_skills};
pub use targets::agent_skills_dir_from_env;

/// Whether an install whose tree is up to date still has agent skills to
/// link, because an approved skill's target directories changed since the
/// links in `.modules.yaml` were made. Reads nothing unless a skill is
/// approved.
pub(crate) fn agent_skill_targets_changed(config: &Config, workspace_root: &Path) -> bool {
    if config.agent_skills_disabled || !config.allow_skills.values().any(|allowed| *allowed) {
        return false;
    }
    let Ok(Some(modules)) =
        pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(&config.modules_dir)
    else {
        return false;
    };
    let linked = modules.linked_skills.unwrap_or_default();
    !linked.is_empty()
        && targets::targets_changed(config, workspace_root, agent_skills_dir_from_env(), &linked)
}

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
    path::{Component, Path, PathBuf},
};

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
    /// Where the hoisted linker placed each dep path, as recorded in
    /// `.modules.yaml`. Read only under that linker.
    pub hoisted_locations: Option<&'a BTreeMap<String, Vec<String>>>,
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
    #[diagnostic(
        code(ERR_PNPM_AGENT_SKILL_NAME_COLLISION),
        help("Deny the skills of one of the packages in the permissions setting.")
    )]
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
    if input.config.agent_skills_disabled
        || input.config.skills_dirs.as_ref().is_some_and(Vec::is_empty)
    {
        prune(input.workspace_root, input.linked, &BTreeSet::new())?;
        update_gitignores(input.workspace_root, input.linked, &BTreeMap::new())?;
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
    materialize(input.workspace_root, input.linked, &desired, input.config.virtual_store_dir())?;
    update_gitignores(input.workspace_root, input.linked, &desired)?;
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
            let name = skill.clone();
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

pub(super) fn recorded_path(workspace_root: &Path, path: &Path) -> String {
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
        .filter(|entry| is_within_workspace(entry))
    {
        let path = workspace_root.join(entry);
        if links_to_a_package_skill(&path) {
            remove_link(&path).map_err(|source| AgentSkillsError::Io { path, source })?;
        }
    }
    Ok(())
}

/// `.modules.yaml` is not trusted to name only pnpm's entries, so pnpm
/// touches only a relative entry reached without `..`.
fn is_within_workspace(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether `path` is a link to `node_modules/**/skills/<skill>`, as every
/// link pnpm creates is, even after `node_modules` is removed.
fn links_to_a_package_skill(path: &Path) -> bool {
    let Ok(target) = fs::read_link(path) else { return false };
    let in_skills_dir = target
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "skills");
    in_skills_dir
        && target
            .components()
            .any(|component| component.as_os_str() == "node_modules")
}

/// Remove a link or junction without following it. Anything else at the path
/// was put there by someone other than pnpm and is left alone.
fn remove_link(path: &Path) -> io::Result<()> {
    match pnpm_fs::is_symlink_or_junction(path) {
        Ok(true) => pnpm_fs::remove_symlink_dir(path),
        Ok(false) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn materialize(
    workspace_root: &Path,
    linked: &[String],
    desired: &BTreeMap<String, PathBuf>,
    virtual_store_dir: &Path,
) -> Result<(), AgentSkillsError> {
    for (entry, target) in desired {
        let path = workspace_root.join(entry);
        if is_occupied(&path, target, linked.contains(entry), virtual_store_dir) {
            return Err(AgentSkillsError::Occupied { path });
        }
    }
    for (entry, target) in desired {
        let path = workspace_root.join(entry);
        create_link(target, &path).map_err(|source| AgentSkillsError::Io { path, source })?;
    }
    Ok(())
}

/// Whether `path` holds something pnpm must not replace. pnpm replaces only
/// a link: one it recorded, one that already resolves to `target`, one that
/// resolves to nothing, as a link into a removed `node_modules` does, or one
/// that resolves to the same skill of another copy of the package in
/// `virtual_store_dir`, such as a peer variant that has left the lockfile.
fn is_occupied(path: &Path, target: &Path, recorded: bool, virtual_store_dir: &Path) -> bool {
    if fs::symlink_metadata(path).is_err() {
        return false;
    }
    let replaceable = pnpm_fs::is_symlink_or_junction(path).unwrap_or(false)
        && (recorded
            || links_to(path, target)
            || fs::metadata(path).is_err()
            || links_to_same_skill_in(path, target, virtual_store_dir));
    !replaceable
}

/// The link target is relative and `target` is canonical, so the link is
/// placed through the canonical form of its directory. Otherwise a symlink
/// in the workspace path, such as `/var` on macOS, leaves it dangling.
fn create_link(target: &Path, path: &Path) -> io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    pnpm_fs::force_symlink_dir(target, &fs::canonicalize(dir)?.join(name)).map(drop)
}

/// Whether `path` resolves inside `virtual_store_dir` to a path ending in
/// the `node_modules/<name>/skills/<skill>` of `target`.
fn links_to_same_skill_in(path: &Path, target: &Path, virtual_store_dir: &Path) -> bool {
    let Some(skill_path) = skill_path_from_modules_dir(target) else { return false };
    matches!(
        (fs::canonicalize(path), fs::canonicalize(virtual_store_dir)),
        (Ok(resolved), Ok(store)) if resolved.starts_with(&store) && resolved.ends_with(skill_path),
    )
}

/// `node_modules/<name>/skills/<skill>` of `<dir>/node_modules/<name>/skills/<skill>`.
fn skill_path_from_modules_dir(target: &Path) -> Option<&Path> {
    let package_dir = target.parent()?.parent()?;
    let modules_dir = package_dir
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name == "node_modules")
        })?;
    target
        .strip_prefix(modules_dir.parent()?)
        .ok()
}

fn links_to(path: &Path, target: &Path) -> bool {
    matches!(
        (fs::canonicalize(path), fs::canonicalize(target)),
        (Ok(resolved), Ok(target)) if resolved == target,
    )
}

/// The ignore rule that older pnpm versions added for links named
/// `pnpm-<package>-<skill>`.
const LEGACY_GITIGNORE_RULE: &str = "pnpm-*";

/// Keep pnpm's entries out of commits, because they point into
/// `node_modules` and would dangle in a fresh clone. Each agent skill
/// directory's `.gitignore` lists the entries linked into it, and drops the
/// ones that were pruned. Other lines are kept.
fn update_gitignores(
    workspace_root: &Path,
    linked: &[String],
    desired: &BTreeMap<String, PathBuf>,
) -> Result<(), AgentSkillsError> {
    let mut by_dir: BTreeMap<PathBuf, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    let entries = linked
        .iter()
        .filter(|entry| !desired.contains_key(*entry))
        .filter(|entry| is_within_workspace(entry))
        .map(|entry| (entry, false))
        .chain(
            desired
                .keys()
                .map(|entry| (entry, true)),
        );
    for (entry, keep) in entries {
        let path = Path::new(entry);
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { continue };
        let (kept, pruned) = by_dir
            .entry(workspace_root.join(dir))
            .or_default();
        let rule = gitignore_rule(&name.to_string_lossy());
        let rules = if keep { kept } else { pruned };
        rules.insert(rule);
    }
    for (dir, (kept, pruned)) in by_dir {
        update_gitignore(&dir.join(".gitignore"), &kept, &pruned)?;
    }
    Ok(())
}

/// Rewrite the `.gitignore` at `path` to list every rule of `kept` and no
/// rule of `pruned`. The file is created only when there is a rule to keep,
/// and removed when no line remains.
fn update_gitignore(
    path: &Path,
    kept: &BTreeSet<String>,
    pruned: &BTreeSet<String>,
) -> Result<(), AgentSkillsError> {
    let io_error = |source| AgentSkillsError::Io { path: path.to_path_buf(), source };
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(io_error(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cannot write through a symlinked .gitignore",
        )));
    }
    let existing = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound && kept.is_empty() => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(io_error(source)),
    };
    let mut lines: Vec<&str> = existing
        .lines()
        .filter(|line| {
            let line = line.trim_end();
            line != LEGACY_GITIGNORE_RULE && !pruned.contains(line)
        })
        .collect();
    let present: BTreeSet<&str> = lines
        .iter()
        .map(|line| line.trim_end())
        .collect();
    let missing: Vec<&str> = kept
        .iter()
        .map(String::as_str)
        .filter(|rule| !present.contains(rule))
        .collect();
    lines.extend(missing);
    let updated: String = lines
        .iter()
        .flat_map(|line| [*line, "\n"])
        .collect();
    if updated == existing {
        return Ok(());
    }
    if updated.is_empty() {
        return fs::remove_file(path).map_err(io_error);
    }
    fs::write(path, updated).map_err(io_error)
}

/// The `.gitignore` rule that matches exactly the entry `name` of its
/// directory.
fn gitignore_rule(name: &str) -> String {
    let mut rule = String::from("/");
    for character in name.chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']' | ' ' | '!' | '#') {
            rule.push('\\');
        }
        rule.push(character);
    }
    rule
}

#[cfg(test)]
mod tests;
