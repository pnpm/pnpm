pub(crate) mod decisions;
mod global_approvals;
pub(crate) mod render;
mod skills_review;

pub(crate) use decisions::Approval;
pub(crate) use render::render_permissions;

mod repository_scope;

use crate::{
    State,
    cli_args::{
        approve_builds::{
            ApprovalStore,
            ApproveBuildsArgs,
            confirm,
            disambiguated_labels,
            partition_params,
            prompt_for_choices,
            retain_undecided_builds,
        },
        ignored_builds::{
            IgnoredBuildsScan,
            get_automatically_ignored_builds,
        },
        rebuild::{
            RebuildSelection,
            run_rebuild,
        },
    },
    confirm_prompt::PromptAvailability,
};
use clap::{
    Args,
    Subcommand,
};
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_config::{
    Config,
    PermissionCapability,
};
use pnpm_package_manager::{
    resync_installed_agent_skills,
    skill_approval_key,
};
use pnpm_reporter::Reporter;
use pnpm_workspace_manifest_writer::set_skills_dirs;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
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
    /// package whose skills are approved, which of its skills. To approve
    /// a Git package across updates, select its repository in the prompt or
    /// pass `<name>@<git-url>` without a commit.
    pub packages: Vec<String>,

    /// Approve everything awaiting approval without interactive prompts.
    #[clap(long)]
    pub all: bool,

    /// Approve permissions for globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
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

impl PendingPermissions {
    /// Drop the builds `config` denies. A run that names no packages decides
    /// only what awaits a decision, so it never turns a denial into an
    /// approval.
    fn exclude_denied_builds(&mut self, config: &Config) -> miette::Result<()> {
        let mut undecided: Vec<String> = self.by_package
            .iter()
            .filter(|(_, capabilities)| capabilities.contains(&PermissionCapability::Build))
            .map(|(pkg, _)| pkg.clone())
            .collect();
        retain_undecided_builds(&mut undecided, self.scan.modules_manifest.as_ref(), config)?;
        self.by_package.retain(|pkg, capabilities| {
            if !undecided.contains(pkg) {
                capabilities.remove(&PermissionCapability::Build);
            }
            !capabilities.is_empty()
        });
        Ok(())
    }
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
            .entry(skill_approval_key(dep_path.as_str()))
            .or_default()
            .insert(PermissionCapability::Skills);
    }
    Ok(PendingPermissions { by_package, scan })
}

use self::skills_review::review_skills;
use decisions::{
    capability_list,
    clear_decided,
    confirmation,
    decisions_by_approval,
    decisions_for_all,
    named_decisions,
    pending_builds_approved,
    write_decisions,
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
        ApproveBuildsArgs { packages: self.packages.clone(), all: self.all, global: self.global }
            .validate()?;
        if self.global {
            return self.run_global::<Reporter>(config).await;
        }
        let mut pending = pending_permissions(config)?;
        if self.packages.is_empty() {
            pending.exclude_denied_builds(config)?;
        }
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

        let approved_config =
            ApprovalStore::Workspace(&settings_dir).config_with_approvals(config)?;
        if approval.decisions.values().any(|decided| decided.skills.is_some()) {
            resync_installed_agent_skills(approved_config, manifest_dir)?;
        }
        let build_packages = pending_builds_approved(&approval.decisions, &pending.by_package);
        if build_packages.is_empty() {
            return Ok(());
        }
        let rebuild_state = State::init(manifest_path.to_path_buf(), approved_config, true)
            .wrap_err("initialize the rebuild state")?;
        let selection =
            RebuildSelection { snapshots: None, names: Some(build_packages), projects: Vec::new() };
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
        self.decide_with_prompt::<Reporter>(
            pending,
            config,
            manifest_dir,
            PromptAvailability::detect(config.ci),
        )
    }

    fn decide_with_prompt<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
        config: &'static Config,
        manifest_dir: &Path,
        prompt: PromptAvailability,
    ) -> miette::Result<Option<Approval>> {
        if !self.packages.is_empty() {
            return Ok(Some(Approval {
                decisions: named_decisions::<Reporter>(&self.packages, pending)?,
                skills_dirs: Vec::new(),
            }));
        }
        if self.all {
            return Ok(Some(Approval {
                decisions: decisions_for_all(pending),
                skills_dirs: Vec::new(),
            }));
        }
        if pending.is_empty() {
            return Ok(None);
        }
        if !prompt.is_available() {
            return Err(ApproveNonInteractive.into());
        }
        let Some(approved) = repository_scope::prompt_for_packages(pending)? else {
            return Ok(None);
        };
        let pending = repository_scope::group_requests(pending.clone(), &approved);
        let Some(review) = review_skills(config, manifest_dir, &approved, &pending)? else {
            return Ok(None);
        };
        let decisions = decisions_by_approval(&approved, &pending, &review.skills_by_package);
        Ok(Some(Approval { decisions, skills_dirs: review.dirs }))
    }
}

pub(crate) fn package_choices(pending: &BTreeMap<String, Capabilities>) -> Vec<(String, String)> {
    let labels = disambiguated_labels(pending.keys().map(String::as_str));
    pending
        .iter()
        .map(|(pkg, capabilities)| {
            (pkg.clone(), format!("{}  {}", labels[pkg.as_str()], capability_list(capabilities)))
        })
        .collect()
}

fn prompt_for_packages(
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<Vec<String>>> {
    prompt_for_choices("Choose which packages to approve", &package_choices(pending))
}

#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[display(r#""pnpm approve" without package arguments or --all requires an interactive terminal"#)]
#[diagnostic(
    code(ERR_PNPM_APPROVE_NON_INTERACTIVE),
    help(
        r#"Run "pnpm approve" in an interactive terminal outside CI, pass --all, or pass the package names to approve (<pkg>) or deny (!<pkg>)."#
    )
)]
pub(super) struct ApproveNonInteractive;

#[cfg(test)]
mod tests;
