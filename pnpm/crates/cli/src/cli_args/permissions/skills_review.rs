use super::Capabilities;
use crate::{
    cli_args::approve_builds::{
        disambiguated_labels,
        prompt_for_choices,
    },
    confirm_prompt::PromptAvailability,
};
use miette::{
    Context,
    Report,
};
use pnpm_config::{
    Config,
    PermissionCapability,
    SkillsPermission,
};
use pnpm_package_manager::skill_approval_options;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::Path,
};

/// The interactive skills review of an approval: the first-run
/// `skills.dirs` prompt, and one skill checkbox per approved package.
#[derive(Default)]
pub(super) struct SkillsReview {
    /// The `skills` decision per package, overriding the all-or-nothing one
    /// the package selection would write.
    pub(super) skills_by_package: BTreeMap<String, SkillsPermission>,
    /// The `skills.dirs` answer, or empty when the user picked none,
    /// recording nothing keeps the detected directories.
    pub(super) dirs: Vec<String>,
}

pub(super) fn review_skills(
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

    let labels = disambiguated_labels(pending.keys().map(String::as_str));
    let Some(skills_by_package) =
        collect_package_skill_decisions(&skills_pending, &labels, &options.skills_by_key)?
    else {
        return Ok(None);
    };

    let Some(dirs) = resolve_skills_dirs(config, &skills_by_package, &options.detected_dirs)?
    else {
        return Ok(None);
    };

    Ok(Some(SkillsReview { skills_by_package, dirs }))
}

fn collect_package_skill_decisions(
    skills_pending: &[&String],
    labels: &BTreeMap<&str, String>,
    skills_by_key: &BTreeMap<String, Vec<String>>,
) -> miette::Result<Option<BTreeMap<String, SkillsPermission>>> {
    let mut skills_by_package = BTreeMap::new();
    for &pkg in skills_pending {
        let skills = skills_by_key
            .get(pkg.as_str())
            .cloned()
            .unwrap_or_default();
        match prompt_for_package_skills(&labels[pkg.as_str()], &skills)? {
            SkillPromptOutcome::Cancelled => return Ok(None),
            SkillPromptOutcome::Undiscovered => {}
            SkillPromptOutcome::Decided(decision) => {
                skills_by_package.insert(pkg.clone(), decision);
            }
        }
    }
    Ok(Some(skills_by_package))
}

fn resolve_skills_dirs(
    config: &Config,
    skills_by_package: &BTreeMap<String, SkillsPermission>,
    detected_dirs: &[String],
) -> miette::Result<Option<Vec<String>>> {
    if config.skills_dirs.is_none()
        && skills_by_package.values().any(SkillsPermission::links_any)
        && !detected_dirs.is_empty()
        && PromptAvailability::detect(config.ci).is_available()
    {
        return prompt_for_skills_dirs(detected_dirs);
    }
    Ok(Some(Vec::new()))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SkillPromptOutcome {
    /// The user selected skills or unselected all of them, or the package
    /// ships one skill, which approving the package approves.
    Decided(SkillsPermission),
    /// No skills were discovered for this package, so it cannot be reviewed.
    Undiscovered,
    /// The user cancelled the prompt.
    Cancelled,
}

pub(super) fn prompt_for_package_skills(
    label: &str,
    skills: &[String],
) -> miette::Result<SkillPromptOutcome> {
    match skills.len() {
        0 => return Ok(SkillPromptOutcome::Undiscovered),
        1 => return Ok(SkillPromptOutcome::Decided(SkillsPermission::All(true))),
        _ => {}
    }
    let choices: Vec<(String, String)> = skills
        .iter()
        .map(|skill| (skill.clone(), skill.clone()))
        .collect();
    let prompt = format!("Choose which of {label}'s agent skills to link");
    let Some(selected) = prompt_for_choices(&prompt, &choices)? else {
        return Ok(SkillPromptOutcome::Cancelled);
    };
    let decision = if selected.is_empty() {
        SkillsPermission::All(false)
    } else {
        SkillsPermission::Chosen(selected)
    };
    Ok(SkillPromptOutcome::Decided(decision))
}

fn prompt_for_skills_dirs(detected_dirs: &[String]) -> miette::Result<Option<Vec<String>>> {
    let choices: Vec<(String, String)> = detected_dirs
        .iter()
        .map(|dir| (dir.clone(), dir.clone()))
        .collect();
    prompt_for_choices("Choose where the approved agent skills are linked", &choices)
}

pub(super) fn review_skills_with_options(
    config: &'static Config,
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
    options: &pnpm_package_manager::SkillApprovalOptions,
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
    let labels = disambiguated_labels(pending.keys().map(String::as_str));
    let Some(skills_by_package) =
        collect_package_skill_decisions(&skills_pending, &labels, &options.skills_by_key)?
    else {
        return Ok(None);
    };
    let Some(dirs) = resolve_skills_dirs(config, &skills_by_package, &options.detected_dirs)?
    else {
        return Ok(None);
    };
    Ok(Some(SkillsReview { skills_by_package, dirs }))
}
