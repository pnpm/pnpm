use super::{
    Approval,
    ApproveArgs,
    ApproveNonInteractive,
    Capabilities,
    decisions::{
        PackageDecisions,
        clear_decided,
        confirmation,
        decisions_by_approval,
        decisions_for_all,
        named_decisions,
        pending_builds_approved,
    },
    pending_permissions,
    prompt_for_packages,
    skills_review::review_skills_with_options,
};
use crate::{
    State,
    cli_args::{
        approve_builds::{
            ApprovalStore,
            confirm,
        },
        global::{
            GlobalError,
            apply_global_build_policy,
            global_approval_store,
            global_group_config,
            is_subdir,
            scan_global_packages,
            warn_about_legacy_global_settings,
        },
        ignored_builds::IgnoredBuildsScan,
        rebuild::{
            RebuildSelection,
            run_rebuild,
        },
    },
    confirm_prompt::PromptAvailability,
};
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_config::{
    Config,
    GetHomeDir,
};
use pnpm_package_manager::{
    resync_installed_agent_skills,
    skill_approval_options,
};
use pnpm_reporter::Reporter;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::{
        Path,
        PathBuf,
    },
};

impl ApproveArgs {
    pub(super) async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        base_config: &'static Config,
    ) -> miette::Result<()> {
        let global_pkg_dir =
            base_config.global_pkg_dir.as_ref().ok_or(GlobalError::NoGlobalBinDir)?;
        warn_about_legacy_global_settings::<Reporter>(base_config, global_pkg_dir)?;
        let (groups, pending) = scan_global_pending_permissions(base_config, global_pkg_dir)?;
        if pending.by_package.is_empty() && self.packages.is_empty() {
            pnpm_diagnostics::println!("There are no packages awaiting approval");
            return Ok(());
        }
        let home_dir = home_dir()?;
        let approval_config =
            global_approval_config(base_config, global_pkg_dir, home_dir.clone())?;
        let skill_options =
            global_skill_approval_options(base_config, &groups, global_pkg_dir, &home_dir)?;
        let Some(approval) =
            self.decide_global::<Reporter>(&pending.by_package, approval_config, &skill_options)?
        else {
            return Ok(());
        };
        let store = global_approval_store(base_config, global_pkg_dir)?;
        store.write_permission_decisions(&approval.decisions)?;
        if !approval.skills_dirs.is_empty() {
            store.write_skills_dirs(&approval.skills_dirs)?;
        }
        rebuild_approved_global_groups::<Reporter>(
            base_config,
            global_pkg_dir,
            store,
            groups,
            &approval.decisions,
        )
        .await
    }
}

fn home_dir() -> miette::Result<PathBuf> {
    pnpm_config::Host::home_dir()
        .ok_or_else(|| miette::miette!("unable to determine home directory"))
}

/// `base_config` with the global build policy applied and agent skills
/// targeting the user's home directory, as the approval prompts read it.
fn global_approval_config(
    base_config: &Config,
    global_pkg_dir: &Path,
    home_dir: PathBuf,
) -> miette::Result<&'static Config> {
    let mut config = base_config.clone();
    config.agent_skills_target_root = Some(home_dir);
    apply_global_build_policy(&mut config, global_pkg_dir)?;
    Ok(Config::leak(config))
}

/// Clear the decisions from every group, relink the approved skills, and
/// rebuild the global packages approved to build.
async fn rebuild_approved_global_groups<Reporter: self::Reporter + 'static>(
    base_config: &Config,
    global_pkg_dir: &Path,
    store: ApprovalStore<'_>,
    groups: Vec<(PathBuf, IgnoredBuildsScan)>,
    decisions: &BTreeMap<String, PackageDecisions>,
) -> miette::Result<()> {
    let mut rebuild_groups = Vec::new();
    for (install_dir, scan) in groups {
        let config = global_group_config(
            base_config,
            &install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let group_pending = pending_permissions(&config)?;
        let build_packages = pending_builds_approved(decisions, &group_pending.by_package);
        clear_decided(scan, decisions)?;
        if decisions.values().any(|decided| decided.skills.is_some()) {
            let approved_config = store.config_with_approvals(&config)?;
            resync_installed_agent_skills(approved_config, &install_dir)?;
        }
        if !build_packages.is_empty() {
            rebuild_groups.push((install_dir, build_packages));
        }
    }
    for (install_dir, build_packages) in rebuild_groups {
        let group_config = global_group_config(
            base_config,
            &install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let config = store.config_with_approvals(&group_config)?;
        let state = State::init(install_dir.join("package.json"), config, true)
            .wrap_err("initialize the global approve state")?;
        let selection =
            RebuildSelection { names: Some(build_packages), projects: Vec::new(), snapshots: None };
        Box::pin(run_rebuild::<Reporter>(&state, selection, None)).await?;
    }
    Ok(())
}

struct GlobalPendingPermissions {
    by_package: BTreeMap<String, Capabilities>,
}

fn scan_global_pending_permissions(
    base_config: &Config,
    global_pkg_dir: &Path,
) -> miette::Result<(Vec<(std::path::PathBuf, IgnoredBuildsScan)>, GlobalPendingPermissions)> {
    let packages =
        scan_global_packages(global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
    let mut groups = Vec::new();
    let mut by_package: BTreeMap<String, Capabilities> = BTreeMap::new();
    let canonical_global_pkg_dir = (!packages.is_empty())
        .then(|| dunce::canonicalize(global_pkg_dir))
        .transpose()
        .into_diagnostic()
        .wrap_err("resolve the global packages directory")?;
    for package in packages
        .into_iter()
        .filter(|package| {
            canonical_global_pkg_dir
                .as_ref()
                .is_none_or(|root| is_subdir(root, &package.install_dir))
        })
    {
        let config = global_group_config(
            base_config,
            &package.install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let pending = pending_permissions(&config)?;
        for (pkg, caps) in &pending.by_package {
            by_package
                .entry(pkg.clone())
                .or_default()
                .extend(caps.iter().copied());
        }
        groups.push((package.install_dir, pending.scan));
    }
    Ok((groups, GlobalPendingPermissions { by_package }))
}

fn global_skill_approval_options(
    base_config: &Config,
    groups: &[(PathBuf, IgnoredBuildsScan)],
    global_pkg_dir: &Path,
    home_dir: &Path,
) -> miette::Result<pnpm_package_manager::SkillApprovalOptions> {
    let mut skills_by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (install_dir, _) in groups {
        let Some(options) = group_skill_approval_options(
            base_config,
            install_dir,
            global_pkg_dir,
            home_dir.to_path_buf(),
        ) else {
            continue;
        };
        for (key, skills) in options.skills_by_key {
            skills_by_key
                .entry(key)
                .or_default()
                .extend(skills);
        }
    }
    for skills in skills_by_key.values_mut() {
        skills.sort();
        skills.dedup();
    }
    Ok(pnpm_package_manager::SkillApprovalOptions {
        skills_by_key,
        detected_dirs: pnpm_package_manager::detected_agent_dirs(home_dir),
    })
}

/// The skill approvals one group's installed packages are waiting for. A
/// group whose configuration or installed state cannot be read has none.
fn group_skill_approval_options(
    base_config: &Config,
    install_dir: &Path,
    global_pkg_dir: &Path,
    home_dir: PathBuf,
) -> Option<pnpm_package_manager::SkillApprovalOptions> {
    let mut config = global_group_config(
        base_config,
        install_dir,
        global_pkg_dir,
        base_config.supported_architectures.clone(),
    )
    .ok()?;
    config.agent_skills_target_root = Some(home_dir);
    skill_approval_options(&config, install_dir).ok()
}

impl ApproveArgs {
    pub(super) fn decide_global<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
        config: &'static Config,
        skill_options: &pnpm_package_manager::SkillApprovalOptions,
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
        if !PromptAvailability::detect(config.ci).is_available() {
            return Err(ApproveNonInteractive.into());
        }
        let Some(selected) = prompt_for_packages(pending)? else {
            return Ok(None);
        };
        let approved: BTreeSet<String> = pending
            .keys()
            .filter(|pkg| selected.contains(pkg))
            .cloned()
            .collect();
        let Some(review) = review_skills_with_options(config, &approved, pending, skill_options)?
        else {
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
