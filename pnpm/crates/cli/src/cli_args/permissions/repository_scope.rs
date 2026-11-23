use super::{
    Capabilities,
    confirm,
    decisions::{
        Decisions,
        confirmation,
    },
    prompt_for_choices,
};
use pnpm_config::SkillsPermission;
use pnpm_package_name::git_repo_allow_build_key_from_dep_path;
use std::{
    borrow::Cow,
    collections::{
        BTreeMap,
        BTreeSet,
    },
};

pub(super) fn confirm_decisions(
    decisions: Decisions,
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<Decisions>> {
    if approved.is_empty() {
        return Ok(Some(decisions));
    }
    let choices = repository_choices(&decisions, approved);
    let repositories = if choices.is_empty() {
        Vec::new()
    } else {
        let Some(selected) = prompt_for_choices(
            "Apply these decisions to all commits from the selected repositories (leave unchecked for current commits only)",
            &choices,
        )?
        else {
            return Ok(None);
        };
        selected
    };
    let message = confirmation(approved, pending, &repositories);
    if !confirm(&message)? {
        return Ok(None);
    }
    Ok(Some(apply_scope(decisions, &repositories)))
}

/// Offer one repository decision only when every pending commit was selected
/// with the same build and skills decisions.
pub(super) fn repository_choices(
    decisions: &Decisions,
    approved: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for (key, decision) in decisions {
        if let Some(repo) = git_repo_allow_build_key_from_dep_path(key) {
            groups
                .entry(repo)
                .or_default()
                .push((key, decision));
        }
    }
    groups
        .into_iter()
        .filter_map(|(repo, members)| {
            let (_, first) = members.first()?;
            let allows_capability = first.build.unwrap_or(false)
                || first.skills.as_ref().is_some_and(SkillsPermission::links_any);
            (allows_capability
                && members
                    .iter()
                    .all(|(key, decision)| approved.contains(*key) && decision == first))
            .then(|| (repo.to_string(), repo.into_owned()))
        })
        .collect()
}

pub(super) fn apply_scope(decisions: Decisions, repositories: &[String]) -> Decisions {
    if repositories.is_empty() {
        return decisions;
    }
    decisions
        .into_iter()
        .map(|(key, decision)| {
            let repo = git_repo_allow_build_key_from_dep_path(&key)
                .filter(|repo| {
                    repositories
                        .iter()
                        .any(|choice| choice == repo.as_ref())
                })
                .map(Cow::into_owned);
            (repo.unwrap_or(key), decision)
        })
        .collect()
}
