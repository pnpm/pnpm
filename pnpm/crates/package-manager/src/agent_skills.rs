//! Agent skills shipped by direct dependencies.
//!
//! A skill is a directory holding a `SKILL.md`, directly under the
//! package's `skills/` or inside category directories below it. pnpm never
//! reads a skill: it finds the directories, records the packages whose
//! skills await approval, and links the approved ones into the project's
//! agent skill directories. A skill is linked under its own name, so
//! agents can invoke it as the skill's ID; when two approved packages ship
//! the same skill name, or the name is already taken in an agent skill
//! directory, the entry is `pnpm-<package>-<skill>` instead.

pub(crate) use installed::resync_agent_skills_at;
pub use installed::{
    ResyncAgentSkillsError, SkillApprovalError, SkillApprovalOptions,
    resync_installed_agent_skills, skill_approval_options,
};
pub use targets::{agent_skills_dir_from_env, detected_agent_dirs};

/// Whether an install whose tree is up to date still has agent skills to
/// link, because an approved skill's target directories changed since the
/// links in `.modules.yaml` were made. Reads nothing unless a skill is
/// approved.
pub(crate) fn agent_skill_targets_changed(config: &Config, workspace_root: &Path) -> bool {
    if config.agent_skills_disabled
        || !config.allow_skills.values().any(SkillsPermission::links_any)
    {
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
mod gitignore;
mod installed;
mod targets;

use crate::{SkillsPolicy, VersionPolicyError};
use derive_more::{Display, Error};
use discovery::{Skill, SkillSource, discover_skill_sources};
use gitignore::update_gitignores;
use miette::Diagnostic;
use pnpm_config::{Config, SkillsPermission};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use std::{
    collections::{BTreeMap, BTreeSet, btree_map},
    fs, io,
    path::{Component, Path, PathBuf},
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

/// Tell the user which packages' skills await approval, by their short
/// label ([`crate::short_package_label`]) rather than the dep path: the approval
/// key of a git package is a URL. Peer variants of one package dedupe to
/// one label.
pub(crate) fn report_pending_skills<Reporter: pnpm_reporter::Reporter>(dep_paths: Vec<String>) {
    if dep_paths.is_empty() {
        return;
    }
    let package_names: BTreeSet<String> = dep_paths
        .into_iter()
        .map(|dep_path| crate::short_package_label(dep_path.as_str()))
        .collect();
    Reporter::emit(&pnpm_reporter::LogEvent::PendingSkills(pnpm_reporter::PendingSkillsLog {
        level: pnpm_reporter::LogLevel::Debug,
        package_names: package_names.into_iter().collect(),
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
        update_gitignores(input.workspace_root, &[], input.linked, &BTreeMap::new())?;
        return Ok(AgentSkillsState::default());
    }
    let sources = discover_skill_sources(input);
    let policy =
        SkillsPolicy::from_approvals(&input.config.allow_skills).map_err(AgentSkillsError::Policy)?;
    let (pending_sources, approved) = decide_sources(sources, &policy);
    let dirs = if approved.is_empty() {
        Vec::new()
    } else {
        targets::target_dirs(input.config, input.workspace_root, input.agent_dir)
            .map_err(|(path, source)| AgentSkillsError::Io { path, source })?
    };
    if !approved.is_empty() && dirs.is_empty() {
        return Err(AgentSkillsError::NoTargetDir { packages: package_list(&approved) });
    }
    let entries = link_entries(input, &dirs, &approved)?;
    let desired = desired_entries(input.workspace_root, &dirs, &entries);
    prune(input.workspace_root, input.linked, &desired.keys().cloned().collect())?;
    materialize(input.workspace_root, input.linked, &desired, input.config.virtual_store_dir())?;
    update_gitignores(input.workspace_root, &dirs, input.linked, &desired)?;
    let mut pending: Vec<String> = pending_sources
        .into_iter()
        .map(|source| source.dep_path)
        .collect();
    pending.sort();
    Ok(AgentSkillsState { pending, linked: desired.into_keys().collect() })
}

fn decide_sources(
    sources: impl IntoIterator<Item = SkillSource>,
    policy: &SkillsPolicy,
) -> (Vec<SkillSource>, Vec<SkillSource>) {
    let mut pending = Vec::new();
    let mut approved = Vec::new();
    for source in sources {
        let Some(decision) = policy.check(&source.dep_path) else {
            pending.push(source);
            continue;
        };
        let skills: Vec<Skill> = source.skills
            .iter()
            .filter(|skill| decision.links_skill(&skill.name))
            .cloned()
            .collect();
        if !skills.is_empty() {
            approved.push(SkillSource { skills, ..source });
        }
    }
    (pending, approved)
}

fn package_list(sources: &[SkillSource]) -> String {
    sources
        .iter()
        .map(|source| crate::short_package_label(source.approval_key.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ")
}

/// The entry name of every approved skill, mapped to the skill directory
/// it links to.
///
/// A skill name one approved package ships is the entry name, unless a
/// target directory holds an entry pnpm must not replace there (a link
/// pnpm recorded may be replaced; anything else may not) — then the
/// prefixed name is the entry, in every directory, so the names do not
/// differ across them. A skill name several approved packages ship gets
/// the prefixed form everywhere.
fn link_entries(
    input: &SyncAgentSkills<'_>,
    dirs: &[PathBuf],
    approved: &[SkillSource],
) -> Result<BTreeMap<String, PathBuf>, AgentSkillsError> {
    let by_name = skills_by_name(approved);
    let mut entries: BTreeMap<String, (PathBuf, &str)> = BTreeMap::new();
    for (skill_name, providers) in &by_name {
        insert_skill_entries(input, dirs, skill_name, providers, &mut entries)?;
    }
    Ok(entries
        .into_iter()
        .map(|(name, (target, _))| (name, target))
        .collect())
}

fn skills_by_name(approved: &[SkillSource]) -> BTreeMap<String, Vec<(&SkillSource, &Skill)>> {
    let mut by_name: BTreeMap<String, Vec<(&SkillSource, &Skill)>> = BTreeMap::new();
    for source in approved {
        for skill in &source.skills {
            by_name
                .entry(skill.name.clone())
                .or_default()
                .push((source, skill));
        }
    }
    by_name
}

fn insert_skill_entries<'a>(
    input: &SyncAgentSkills<'_>,
    dirs: &[PathBuf],
    skill_name: &str,
    providers: &[(&'a SkillSource, &'a Skill)],
    entries: &mut BTreeMap<String, (PathBuf, &'a str)>,
) -> Result<(), AgentSkillsError> {
    if providers.len() > 1 {
        for &(source, skill) in providers {
            insert_entry(entries, prefixed_name(source, skill), target_of(source, skill), source)?;
        }
        return Ok(());
    }
    let (source, skill) = providers[0];
    let target = target_of(source, skill);
    let name = if bare_name_free(input, dirs, skill_name, &target) {
        skill_name.to_string()
    } else {
        prefixed_name(source, skill)
    };
    insert_entry(entries, name, target, source)
}

/// The directory an approved skill links to, already resolved to the
/// package's real location.
fn target_of(source: &SkillSource, skill: &Skill) -> PathBuf {
    source.package_dir.join("skills").join(&skill.dir)
}

/// The fallback entry name: `pnpm-<package>-<skill>`.
fn prefixed_name(source: &SkillSource, skill: &Skill) -> String {
    format!("{AGENT_SKILL_LINK_PREFIX}{}-{}", source.link_segment, skill.name)
}

/// Whether every target directory can take an entry named `name`: an
/// absent path is fine and so is a link pnpm recorded, while a link
/// another package made, a real directory, or a file needs the prefixed
/// name.
fn bare_name_free(
    input: &SyncAgentSkills<'_>,
    dirs: &[PathBuf],
    name: &str,
    target: &Path,
) -> bool {
    !dirs
        .iter()
        .any(|dir| {
            let path = input.workspace_root.join(dir).join(name);
            let recorded = input.linked
                .iter()
                .any(|entry| *entry == recorded_path(input.workspace_root, &path));
            is_occupied(&path, target, recorded, input.config.virtual_store_dir())
        })
}

/// Record the entry name with the skill directory it links to, erroring
/// when an entry of this sync already claimed the name.
fn insert_entry<'a>(
    entries: &mut BTreeMap<String, (PathBuf, &'a str)>,
    name: String,
    target: PathBuf,
    source: &'a SkillSource,
) -> Result<(), AgentSkillsError> {
    match entries.entry(name) {
        btree_map::Entry::Vacant(slot) => {
            slot.insert((target, source.dep_path.as_str()));
            Ok(())
        }
        btree_map::Entry::Occupied(occupied) => Err(AgentSkillsError::NameCollision {
            name: occupied.key().clone(),
            first: (*occupied.get().1).to_string(),
            second: source.dep_path.clone(),
        }),
    }
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
        .ancestors()
        .any(|ancestor| {
            ancestor
                .file_name()
                .is_some_and(|name| name == "skills")
        });
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

/// `node_modules/<name>/skills/<skill>` of `<dir>/node_modules/<name>/skills/<skill>`,
/// nested categories included: the canonical form of every copy of the skill
/// in the virtual store ends with it.
fn skill_path_from_modules_dir(target: &Path) -> Option<&Path> {
    let modules_dir = target
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

#[cfg(test)]
mod tests;
