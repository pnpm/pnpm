use super::{
    AgentSkillsError, SyncAgentSkills, agent_skills_dir_from_env, decide_sources,
    detected_agent_dirs, discover_skill_sources, discovery::SkillSource, sync_agent_skills,
};
use crate::{SkillsPolicy, VersionPolicyError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_lockfile::{LoadLockfileError, Lockfile};
use pnpm_modules_yaml::{Host, ReadModulesError, WriteModulesError};
use std::{collections::BTreeMap, path::Path};

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
    let state = sync_agent_skills(&SyncAgentSkills {
        config,
        workspace_root,
        lockfile,
        included: modules.included,
        linked: modules.linked_skills.as_deref().unwrap_or_default(),
        agent_dir: agent_skills_dir_from_env(),
        hoisted_locations: modules.hoisted_locations.as_ref(),
    })
    .map_err(ResyncAgentSkillsError::Sync)?;
    let pending_keys = state.pending_keys();
    let pending_skills = (!state.pending.is_empty()).then(|| {
        state.pending
            .into_iter()
            .map(Into::into)
            .collect()
    });
    let linked_skills = (!state.linked.is_empty()).then_some(state.linked);
    if modules.pending_skills == pending_skills && modules.linked_skills == linked_skills {
        return Ok(pending_keys);
    }
    modules.pending_skills = pending_skills;
    modules.linked_skills = linked_skills;
    pnpm_modules_yaml::write_modules_manifest::<Host>(&config.modules_dir, modules)
        .map_err(ResyncAgentSkillsError::WriteModules)?;
    Ok(pending_keys)
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
    let sources = discover_skill_sources(&SyncAgentSkills {
        config,
        workspace_root: &workspace_root,
        lockfile,
        included: modules.included,
        linked: &[],
        agent_dir: agent_skills_dir_from_env(),
        hoisted_locations: modules.hoisted_locations.as_ref(),
    });
    let policy =
        SkillsPolicy::from_approvals(&config.allow_skills).map_err(SkillApprovalError::Policy)?;
    let (pending, _) = decide_sources(sources, &policy);
    Ok(SkillApprovalOptions {
        skills_by_key: pending_skills_by_key(pending),
        detected_dirs: detected_agent_dirs(&workspace_root),
    })
}

fn pending_skills_by_key(pending: Vec<SkillSource>) -> BTreeMap<String, Vec<String>> {
    pending
        .into_iter()
        .map(|source| {
            (
                source.approval_key,
                source.skills
                    .into_iter()
                    .map(|skill| skill.name)
                    .collect(),
            )
        })
        .collect()
}
