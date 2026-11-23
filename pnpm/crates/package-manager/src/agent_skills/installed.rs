use super::{
    AgentSkillsError,
    SyncAgentSkills,
    agent_skills_dir_from_env,
    decide_sources,
    detected_agent_dirs,
    discover_skill_sources,
    discovery::SkillSource,
    links::canonical_recorded_entry,
    sync_with_other_links,
};
use crate::{
    SkillsPolicy,
    VersionPolicyError,
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_lockfile::{
    LoadLockfileError,
    Lockfile,
};
use pnpm_modules_yaml::{
    Host,
    ReadModulesError,
    WriteModulesError,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::Path,
};

/// Errors from [`resync_installed_agent_skills`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ResyncAgentSkillsError {
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),

    #[diagnostic(transparent)]
    LoadLockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    ReadModules(#[error(source)] ReadModulesError),

    #[diagnostic(transparent)]
    WriteModules(#[error(source)] WriteModulesError),

    #[diagnostic(transparent)]
    Sync(#[error(source)] AgentSkillsError),
}

/// Reconcile the agent skills of what is installed under
/// `config.modules_dir` with `config`'s approvals, and record the result in
/// its `.modules.yaml`. For a command that changes approvals without
/// installing. Does nothing when nothing is installed.
pub fn resync_installed_agent_skills(
    config: &Config,
    manifest_dir: &Path,
) -> Result<(), ResyncAgentSkillsError> {
    let workspace_root = crate::install::lockfile_root_dir(config, manifest_dir)
        .map_err(ResyncAgentSkillsError::FindWorkspaceDir)?;
    resync_agent_skills_at(config, &workspace_root, None).map(drop)
}

/// Reconcile an active global group while preserving other groups' links.
pub fn resync_global_agent_skills(
    config: &Config,
    install_dir: &Path,
    other_links: &BTreeSet<String>,
) -> Result<(), ResyncAgentSkillsError> {
    resync_agent_skills(config, install_dir, None, other_links).map(drop)
}

/// [`resync_installed_agent_skills`] for an install rooted at
/// `workspace_root`, reusing `current_lockfile` when the caller has loaded
/// it. Writes `.modules.yaml` only when the result differs from what it
/// records, and returns the approval keys of the packages whose skills
/// await approval.
pub(crate) fn resync_agent_skills_at(
    config: &Config,
    workspace_root: &Path,
    current_lockfile: Option<&Lockfile>,
) -> Result<Vec<String>, ResyncAgentSkillsError> {
    resync_agent_skills_dep_paths_at(config, workspace_root, current_lockfile)
        .map(|dep_paths| {
            dep_paths
                .iter()
                .map(|dep_path| crate::skill_approval_key(dep_path))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
}

/// Resync installed skills and return the dependency paths awaiting approval.
pub(crate) fn resync_agent_skills_dep_paths_at(
    config: &Config,
    workspace_root: &Path,
    current_lockfile: Option<&Lockfile>,
) -> Result<Vec<String>, ResyncAgentSkillsError> {
    resync_agent_skills(config, workspace_root, current_lockfile, &BTreeSet::new())
}

fn resync_agent_skills(
    config: &Config,
    workspace_root: &Path,
    current_lockfile: Option<&Lockfile>,
    other_links: &BTreeSet<String>,
) -> Result<Vec<String>, ResyncAgentSkillsError> {
    let Some(mut modules) = pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir)
        .map_err(ResyncAgentSkillsError::ReadModules)?
    else {
        return Ok(Vec::new());
    };
    let loaded;
    let lockfile = if let Some(lockfile) = current_lockfile {
        lockfile
    } else {
        loaded = Lockfile::load_current_from_install_state_dir(&config.install_state_dir)
            .map_err(ResyncAgentSkillsError::LoadLockfile)?;
        let Some(lockfile) = loaded.as_ref() else { return Ok(Vec::new()) };
        lockfile
    };
    let target_root = config.agent_skills_target_root.as_deref().unwrap_or(workspace_root);
    let recorded = modules.linked_skills.as_deref().unwrap_or_default();
    let canonical_links;
    let canonical_other_links;
    let (linked, other_links) = if config.agent_skills_target_root.is_some() {
        canonical_links = recorded
            .iter()
            .filter_map(|entry| canonical_recorded_entry(target_root, entry))
            .collect::<Vec<_>>();
        canonical_other_links = other_links
            .iter()
            .filter_map(|entry| canonical_recorded_entry(target_root, entry))
            .collect();
        (canonical_links.as_slice(), &canonical_other_links)
    } else {
        (recorded, other_links)
    };
    let state = sync_with_other_links(
        &SyncAgentSkills {
            config,
            workspace_root,
            lockfile,
            included: modules.included,
            linked,
            agent_dir: agent_skills_dir_from_env(),
            target_root,
            hoisted_locations: modules.hoisted_locations.as_ref(),
        },
        other_links,
    )
    .map_err(ResyncAgentSkillsError::Sync)?;
    let pending_dep_paths = state.pending.clone();
    if state.update_modules(&mut modules) {
        pnpm_modules_yaml::write_modules_manifest::<Host>(&config.modules_dir, modules)
            .map_err(ResyncAgentSkillsError::WriteModules)?;
    }
    Ok(pending_dep_paths)
}

/// Errors from [`skill_approval_options`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SkillApprovalError {
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),

    #[diagnostic(transparent)]
    LoadLockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    ReadModules(#[error(source)] ReadModulesError),

    #[diagnostic(transparent)]
    Policy(#[error(source)] VersionPolicyError),
}

/// The inputs of an agent-skills approval prompt: what each package
/// awaiting approval ships, and where skills can link at all.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SkillApprovalOptions {
    /// The skills each package awaiting approval ships, keyed by the
    /// package's approval key.
    pub skills_by_key: BTreeMap<String, Vec<String>>,
    /// The `skills.dirs` candidates detection yields at the workspace root,
    /// workspace-root-relative, sorted.
    pub detected_dirs: Vec<String>,
}

/// The inputs of an agent-skills approval prompt at the install rooted at
/// `manifest_dir`. Reads what is installed; links and records nothing.
pub fn skill_approval_options(
    config: &Config,
    manifest_dir: &Path,
) -> Result<SkillApprovalOptions, SkillApprovalError> {
    let workspace_root = crate::install::lockfile_root_dir(config, manifest_dir)
        .map_err(SkillApprovalError::FindWorkspaceDir)?;
    let Some(modules) = pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir)
        .map_err(SkillApprovalError::ReadModules)?
    else {
        return Ok(SkillApprovalOptions::default());
    };
    let loaded;
    let lockfile = {
        loaded = Lockfile::load_current_from_install_state_dir(&config.install_state_dir)
            .map_err(SkillApprovalError::LoadLockfile)?;
        let Some(lockfile) = loaded.as_ref() else {
            return Ok(SkillApprovalOptions::default());
        };
        lockfile
    };
    let target_root =
        config.agent_skills_target_root.clone().unwrap_or_else(|| workspace_root.clone());
    let sources = discover_skill_sources(&SyncAgentSkills {
        config,
        workspace_root: &workspace_root,
        lockfile,
        included: modules.included,
        linked: &[],
        agent_dir: agent_skills_dir_from_env(),
        target_root: &target_root,
        hoisted_locations: modules.hoisted_locations.as_ref(),
    });
    let policy =
        SkillsPolicy::from_approvals(&config.allow_skills).map_err(SkillApprovalError::Policy)?;
    let (pending, _) = decide_sources(sources, &policy);
    Ok(SkillApprovalOptions {
        skills_by_key: pending_skills_by_key(pending),
        detected_dirs: detected_agent_dirs(&target_root),
    })
}

fn pending_skills_by_key(pending: Vec<SkillSource>) -> BTreeMap<String, Vec<String>> {
    let mut by_key: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in pending {
        let names = by_key.entry(source.approval_key).or_default();
        for skill in source.skills {
            names.insert(skill.name);
        }
    }
    by_key
        .into_iter()
        .map(|(key, names)| (key, names.into_iter().collect()))
        .collect()
}

#[cfg(test)]
mod tests;
