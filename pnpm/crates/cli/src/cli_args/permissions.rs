pub(crate) mod decisions;
pub(crate) mod render;

pub(crate) use decisions::Approval;
pub(crate) use render::render_permissions;

use crate::{
    State,
    cli_args::{
        approve_builds::{
            ApproveBuildsArgs, config_with_install_approvals, confirm, partition_params,
            prompt_for_choices,
        },
        ignored_builds::{IgnoredBuildsScan, get_automatically_ignored_builds},
        rebuild::{RebuildSelection, run_rebuild},
    },
    confirm_prompt::stdin_is_terminal,
};
use clap::{Args, Subcommand};
use miette::{Context, IntoDiagnostic, Report};
use pnpm_config::{Config, PermissionCapability, SkillsPermission, is_ci};
use pnpm_package_manager::{
    allow_build_key_from_ignored_build, resync_installed_agent_skills, short_package_label,
    skill_approval_options,
};
use pnpm_reporter::{Reporter, emit_global_warning};
use pnpm_workspace_manifest_writer::set_skills_dirs;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Review what dependencies may do: run build scripts (`build`) and provide
/// agent skills (`skills`). With no subcommand, lists the decisions and what
/// awaits approval.
#[derive(Debug, Args)]
pub struct PermissionsArgs {
    #[clap(subcommand)]
    pub command: Option<PermissionsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Approve or deny what the dependencies awaiting approval may do.
    Approve(ApproveArgs),
    /// List the decisions in pnpm-workspace.yaml and what awaits approval.
    #[clap(visible_alias = "ls")]
    List,
}

/// Approve or deny what dependencies may do: run build scripts and provide
/// agent skills.
#[derive(Debug, Args)]
pub struct ApproveArgs {
    /// Packages to approve (`<pkg>`) or deny (`!<pkg>`). A package is
    /// approved or denied for everything it requests. With no packages, the
    /// packages awaiting approval are chosen interactively, and for a
    /// package whose skills are approved, which of its skills.
    pub packages: Vec<String>,

    /// Approve everything awaiting approval without interactive prompts.
    #[clap(long)]
    pub all: bool,
}

type Capabilities = BTreeSet<PermissionCapability>;

/// Every capability, for a package named before it requested any.
const ALL_CAPABILITIES: [PermissionCapability; 2] =
    [PermissionCapability::Build, PermissionCapability::Skills];

/// What awaits approval, by the key a package is approved under.
pub(crate) struct PendingPermissions {
    pub(crate) by_package: BTreeMap<String, Capabilities>,
    scan: IgnoredBuildsScan,
}

/// Read the pending build scripts and agent skills from `.modules.yaml`.
pub(crate) fn pending_permissions(config: &Config) -> miette::Result<PendingPermissions> {
    let scan = get_automatically_ignored_builds(config)?;
    let mut by_package: BTreeMap<String, Capabilities> = BTreeMap::new();
    for name in scan.names.iter().flatten() {
        by_package
            .entry(name.clone())
            .or_default()
            .insert(PermissionCapability::Build);
    }
    let pending_skills = scan.modules_manifest
        .iter()
        .flat_map(|modules| modules.pending_skills.iter().flatten());
    for dep_path in pending_skills {
        by_package
            .entry(allow_build_key_from_ignored_build(dep_path.as_str()))
            .or_default()
            .insert(PermissionCapability::Skills);
    }
    Ok(PendingPermissions { by_package, scan })
}

use decisions::{
    capability_list, clear_decided, confirmation, decisions_by_approval, named_decisions,
    pending_builds_approved, write_decisions,
};

impl ApproveArgs {
    /// Decide, write the decisions to `pnpm-workspace.yaml`, clear them
    /// from `.modules.yaml`, link the approved agent skills, and rebuild the
    /// packages approved to build.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static Config,
        manifest_path: &Path,
    ) -> miette::Result<()> {
        ApproveBuildsArgs { packages: self.packages.clone(), all: self.all, global: false }
            .validate()?;
        let pending = pending_permissions(config)?;
        if pending.by_package.is_empty() && self.packages.is_empty() {
            pnpm_diagnostics::println!("There are no packages awaiting approval");
            return Ok(());
        }
        let manifest_dir = manifest_path.parent().unwrap_or(dir);
        let Some(approval) = self.decide::<Reporter>(&pending.by_package, config, manifest_dir)?
        else {
            return Ok(());
        };
        let settings_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        write_decisions(&settings_dir, &approval.decisions)?;
        if !approval.skills_dirs.is_empty() {
            set_skills_dirs(&settings_dir, &approval.skills_dirs).into_diagnostic()?;
        }
        clear_decided(pending.scan, &approval.decisions)?;

        let approved_config = config_with_install_approvals(config, &settings_dir)?;
        if approval.decisions.values().any(|decided| decided.skills.is_some()) {
            resync_installed_agent_skills(approved_config, manifest_dir)?;
        }
        let build_packages = pending_builds_approved(&approval.decisions, &pending.by_package);
        if build_packages.is_empty() {
            return Ok(());
        }
        let rebuild_state = State::init(manifest_path.to_path_buf(), approved_config, true)
            .wrap_err("initialize the rebuild state")?;
        let selection = RebuildSelection { names: Some(build_packages), projects: Vec::new() };
        Box::pin(run_rebuild::<Reporter>(&rebuild_state, selection, None)).await
    }

    /// The decisions of this run. When packages are named, every capability
    /// they request (or ever will) gets the verdicts they asked for. When
    /// `--all` is set, every pending package is approved for everything it
    /// requests. Otherwise the run prompts, and for a package whose skills
    /// are being approved, prompts for the skills.
    fn decide<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
        config: &'static Config,
        manifest_dir: &Path,
    ) -> miette::Result<Option<Approval>> {
        if !self.packages.is_empty() {
            return Ok(Some(Approval {
                decisions: named_decisions::<Reporter>(&self.packages, pending)?,
                skills_dirs: Vec::new(),
            }));
        }
        if self.all {
            return Ok(Some(Approval {
                decisions: decisions_by_approval(
                    &pending.keys().cloned().collect(),
                    pending,
                    &BTreeMap::new(),
                ),
                skills_dirs: Vec::new(),
            }));
        }
        let Some(selected) = prompt_for_packages(pending)? else {
            return Ok(None);
        };
        let approved: BTreeSet<String> = pending
            .keys()
            .filter(|pkg| selected.contains(pkg))
            .cloned()
            .collect();
        let Some(review) = review_skills(config, manifest_dir, &approved, pending)? else {
            return Ok(None);
        };
        if !approved.is_empty() && !confirm(&confirmation(&approved, pending))? {
            return Ok(None);
        }
        Ok(Some(Approval {
            decisions: decisions_by_approval(&approved, pending, &review.skills_by_package),
            skills_dirs: review.dirs,
        }))
    }
}

fn prompt_for_packages(
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<Vec<String>>> {
    let choices: Vec<(String, String)> = pending
        .iter()
        .map(|(pkg, capabilities)| {
            (
                pkg.clone(),
                format!("{}  {}", short_package_label(pkg), capability_list(capabilities)),
            )
        })
        .collect();
    prompt_for_choices("Choose which packages to approve", &choices)
}

/// The interactive skills review of an approval: the first-run
/// `skills.dirs` prompt, and one skill checkbox per approved package.
#[derive(Default)]
struct SkillsReview {
    /// The `skills` decision per package, overriding the all-or-nothing one
    /// the package selection would write.
    skills_by_package: BTreeMap<String, SkillsPermission>,
    /// The `skills.dirs` answer, or empty when the user picked none —
    /// recording nothing keeps the detected directories.
    dirs: Vec<String>,
}

fn review_skills(
    config: &'static Config,
    manifest_dir: &Path,
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<SkillsReview>> {
    let skills_pending: Vec<&String> = approved
        .iter()
        .filter(|pkg| {
            pending
                .get(*pkg)
                .is_some_and(|capabilities| capabilities.contains(&PermissionCapability::Skills))
        })
        .collect();
    if skills_pending.is_empty() {
        return Ok(Some(SkillsReview::default()));
    }

    let options = skill_approval_options(config, manifest_dir)
        .map_err(Report::new)
        .wrap_err("find the skills of the packages awaiting approval")?;

    let mut skills_by_package: BTreeMap<String, SkillsPermission> = BTreeMap::new();
    for pkg in skills_pending {
        let skills = options.skills_by_key
            .get(pkg.as_str())
            .cloned()
            .unwrap_or_default();
        let Some(decision) = prompt_for_package_skills(pkg, &skills)? else {
            return Ok(None);
        };
        skills_by_package.insert(pkg.clone(), decision);
    }

    let dirs = if config.skills_dirs.is_none()
        && skills_by_package.values().any(SkillsPermission::links_any)
        && !options.detected_dirs.is_empty()
        && can_prompt()
    {
        let Some(selected) = prompt_for_skills_dirs(&options.detected_dirs)? else {
            return Ok(None);
        };
        selected
    } else {
        Vec::new()
    };

    Ok(Some(SkillsReview { skills_by_package, dirs }))
}

fn prompt_for_package_skills(
    pkg: &str,
    skills: &[String],
) -> miette::Result<Option<SkillsPermission>> {
    if skills.is_empty() {
        return Ok(Some(SkillsPermission::All(true)));
    }
    let choices: Vec<(String, String)> = skills
        .iter()
        .map(|skill| (skill.clone(), skill.clone()))
        .collect();
    let prompt = format!("Choose which of {}'s agent skills to link", short_package_label(pkg));
    let Some(selected) = prompt_for_choices(&prompt, &choices)? else {
        return Ok(None);
    };
    let decision = if selected.is_empty() {
        SkillsPermission::All(false)
    } else {
        SkillsPermission::Chosen(selected)
    };
    Ok(Some(decision))
}

fn prompt_for_skills_dirs(detected_dirs: &[String]) -> miette::Result<Option<Vec<String>>> {
    let choices: Vec<(String, String)> = detected_dirs
        .iter()
        .map(|dir| (dir.clone(), dir.clone()))
        .collect();
    prompt_for_choices("Choose where the approved agent skills are linked", &choices)
}

/// Whether the current run can prompt. Under CI, or when the input is not a
/// terminal, a prompt is skipped and answered another time.
fn can_prompt() -> bool {
    !is_ci() && stdin_is_terminal().unwrap_or(false)
}

#[cfg(test)]
mod tests;
