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
    repository_scope::group_requests,
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
            links_of_other_groups,
            scan_global_packages,
            warn_about_legacy_global_settings,
        },
        global_bin_lock::acquire_global_bin_lock,
        ignored_builds::get_automatically_ignored_builds,
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
use pnpm_deps_restorer::SkillsPolicy;
use pnpm_package_manager::{
    resync_global_agent_skills,
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
        let GlobalPendingPermissions { groups, by_package } =
            scan_global_pending_permissions(base_config, global_pkg_dir, self.packages.is_empty())?;
        if by_package.is_empty() && self.packages.is_empty() {
            pnpm_diagnostics::println!("There are no packages awaiting approval");
            return Ok(());
        }
        let home_dir = home_dir()?;
        let approval_config =
            global_approval_config(base_config, global_pkg_dir, home_dir.clone())?;
        let Some(approval) = self.decide_global::<Reporter>(&by_package, &approval_config, || {
            global_skill_approval_options(base_config, &groups, global_pkg_dir, &home_dir)
        })?
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
) -> miette::Result<Config> {
    let mut config = base_config.clone();
    config.agent_skills_target_root = Some(home_dir);
    apply_global_build_policy(&mut config, global_pkg_dir)?;
    Ok(config)
}

/// Reconcile skills and clear each group's ignored builds after rebuilding.
async fn rebuild_approved_global_groups<Reporter: self::Reporter + 'static>(
    base_config: &Config,
    global_pkg_dir: &Path,
    store: ApprovalStore<'_>,
    groups: Vec<PathBuf>,
    decisions: &BTreeMap<String, PackageDecisions>,
) -> miette::Result<()> {
    let sync_skills = decisions.values().any(|decided| decided.skills.is_some());
    let build_decisions: BTreeMap<_, _> = decisions
        .iter()
        .filter_map(|(pkg, decided)| {
            Some((pkg.clone(), PackageDecisions { build: Some(decided.build?), skills: None }))
        })
        .collect();
    for install_dir in groups {
        let config = global_group_config(
            base_config,
            &install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let config = store.config_with_approvals(&config)?;
        if sync_skills {
            reconcile_global_skills(config, global_pkg_dir, &install_dir)?;
        }
        let group_pending = pending_permissions(config)?;
        let build_packages = pending_builds_approved(decisions, &group_pending.by_package);
        if !build_packages.is_empty() {
            let state = State::init(install_dir.join("package.json"), config, true)
                .wrap_err("initialize the global approve state")?;
            let selection = RebuildSelection {
                names: Some(build_packages),
                projects: Vec::new(),
                snapshots: None,
            };
            Box::pin(run_rebuild::<Reporter>(&state, selection, None)).await?;
        }
        if !build_decisions.is_empty() {
            clear_decided(get_automatically_ignored_builds(config)?, &build_decisions)?;
        }
    }
    Ok(())
}

fn reconcile_global_skills(
    config: &Config,
    global_pkg_dir: &Path,
    install_dir: &Path,
) -> miette::Result<()> {
    let global_bin_dir = config.global_bin.as_deref().ok_or(GlobalError::NoGlobalBinDir)?;
    let _lock = acquire_global_bin_lock(global_bin_dir)?;
    let other_links = links_of_other_groups(global_pkg_dir, &BTreeSet::from([install_dir]))?;
    resync_global_agent_skills(config, install_dir, &other_links).map_err(miette::Report::new)
}

/// A saved choice may still need to be applied after an interrupted approval.
fn has_recorded_pending_skills(
    config: &Config,
    pending: &super::PendingPermissions,
) -> miette::Result<bool> {
    let Some(skills) = pending.scan.modules_manifest
        .as_ref()
        .and_then(|modules| modules.pending_skills.as_ref())
    else {
        return Ok(false);
    };
    let policy = SkillsPolicy::from_approvals(&config.allow_skills).into_diagnostic()?;
    Ok(skills
        .iter()
        .any(|dep_path| policy.check(dep_path.as_str()).is_some()))
}

struct GlobalPendingPermissions {
    groups: Vec<PathBuf>,
    by_package: BTreeMap<String, Capabilities>,
}

fn scan_global_pending_permissions(
    base_config: &Config,
    global_pkg_dir: &Path,
    automatic: bool,
) -> miette::Result<GlobalPendingPermissions> {
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
        let pending = if automatic {
            pending_for_automatic_approval(&config, global_pkg_dir, &package.install_dir)?
        } else {
            pending_permissions(&config)?
        };
        for (pkg, caps) in &pending.by_package {
            by_package
                .entry(pkg.clone())
                .or_default()
                .extend(caps.iter().copied());
        }
        groups.push(package.install_dir);
    }
    Ok(GlobalPendingPermissions { groups, by_package })
}

fn pending_for_automatic_approval(
    config: &Config,
    global_pkg_dir: &Path,
    install_dir: &Path,
) -> miette::Result<super::PendingPermissions> {
    let mut pending = pending_permissions(config)?;
    if has_recorded_pending_skills(config, &pending)? {
        reconcile_global_skills(config, global_pkg_dir, install_dir)?;
        pending = pending_permissions(config)?;
        if has_recorded_pending_skills(config, &pending)? {
            let display_install_dir = install_dir.display();
            return Err(miette::miette!(
                "unable to apply saved agent skill decisions for {display_install_dir}",
            ));
        }
    }
    pending.exclude_denied_builds(config)?;
    Ok(pending)
}

fn global_skill_approval_options(
    base_config: &Config,
    groups: &[PathBuf],
    global_pkg_dir: &Path,
    home_dir: &Path,
) -> miette::Result<pnpm_package_manager::SkillApprovalOptions> {
    let mut skills_by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for install_dir in groups {
        let config = global_group_config(
            base_config,
            install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let options = skill_approval_options(&config, install_dir)
            .map_err(miette::Report::new)
            .wrap_err_with(|| format!("find the skills of {}", install_dir.display()))?;
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

impl ApproveArgs {
    pub(super) fn decide_global<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
        config: &Config,
        load_skill_options: impl FnOnce() -> miette::Result<pnpm_package_manager::SkillApprovalOptions>,
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
        approve_global_interactively(pending, config, load_skill_options)
    }
}

fn approve_global_interactively(
    pending: &BTreeMap<String, Capabilities>,
    config: &Config,
    load_skill_options: impl FnOnce() -> miette::Result<pnpm_package_manager::SkillApprovalOptions>,
) -> miette::Result<Option<Approval>> {
    let Some(selected) = super::repository_scope::prompt_for_packages(pending)? else {
        return Ok(None);
    };
    let grouped_pending = group_global_pending(pending, &selected);
    let approved = selected;
    let Some(review) =
        review_skills_with_options(config, &approved, &grouped_pending, load_skill_options)?
    else {
        return Ok(None);
    };
    if !approved.is_empty() && !confirm(&confirmation(&approved, &grouped_pending))? {
        return Ok(None);
    }
    Ok(Some(Approval {
        decisions: decisions_by_approval(&approved, &grouped_pending, &review.skills_by_package),
        skills_dirs: review.dirs,
    }))
}

pub(super) fn group_global_pending(
    pending: &BTreeMap<String, Capabilities>,
    selected: &BTreeSet<String>,
) -> BTreeMap<String, Capabilities> {
    group_requests(pending.clone(), selected)
}
