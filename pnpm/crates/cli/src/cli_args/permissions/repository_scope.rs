use super::{
    Capabilities,
    decisions::Decisions,
    package_choices,
    prompt_for_choices,
};
use crate::checkbox_prompt::{
    CheckboxAnswer,
    CheckboxChoice,
    CheckboxItem,
    CheckboxPrompt,
};
use miette::IntoDiagnostic;
use pnpm_package_name::git_repo_allow_build_key_from_dep_path;
use std::collections::{
    BTreeMap,
    btree_map::Entry,
};

/// Selected pending keys and the policy key under which to save each one.
pub(super) type Selection = BTreeMap<String, String>;

pub(super) fn prompt_for_packages(
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<Selection>> {
    let choices = package_choices(pending);
    let alternatives = repository_alternatives(&choices);
    let message = "Choose which packages to approve";
    if alternatives.is_empty() {
        return Ok(prompt_for_choices(message, &choices)?
            .map(|keys| {
                keys.into_iter()
                    .map(|key| (key.clone(), key))
                    .collect()
            }));
    }
    let items = choices
        .into_iter()
        .enumerate()
        .map(|(index, (key, label))| {
            let description =
                alternatives.contains_key(&index).then(|| "r: select all commits".to_string());
            CheckboxItem::Choice(CheckboxChoice {
                short: label.clone(),
                name: if description.is_some() { format!("{label}  [this commit]") } else { label },
                description,
                value: (key.clone(), key),
            })
        })
        .collect();
    let answer = CheckboxPrompt::new(message, items)
        .with_alternatives(alternatives)
        .interact()
        .into_diagnostic()?;
    Ok(match answer {
        CheckboxAnswer::Selected(selected) => Some(selected.into_iter().collect()),
        CheckboxAnswer::Cancelled => None,
    })
}

fn repository_alternatives(
    choices: &[(String, String)],
) -> BTreeMap<usize, CheckboxChoice<(String, String)>> {
    choices
        .iter()
        .enumerate()
        .filter_map(|(index, (key, label))| {
            let repo = git_repo_allow_build_key_from_dep_path(key)?.into_owned();
            let choice = CheckboxChoice {
                name: format!("{label}  [all commits of {repo}]"),
                short: format!("{label} (all commits)"),
                value: (key.clone(), repo),
                description: Some("r: switch to this commit".to_string()),
            };
            Some((index, choice))
        })
        .collect()
}

pub(super) fn apply_scope(decisions: Decisions, selected: &Selection) -> miette::Result<Decisions> {
    let mut scoped = Decisions::new();
    for (key, decision) in decisions {
        let policy_key = selected
            .get(&key)
            .cloned()
            .unwrap_or(key);
        match scoped.entry(policy_key) {
            Entry::Vacant(entry) => {
                entry.insert(decision);
            }
            Entry::Occupied(entry) if entry.get() != &decision => {
                miette::bail!(
                    "Conflicting decisions for {}. Choose the same permissions for all commits or approve each commit separately.",
                    entry.key(),
                );
            }
            Entry::Occupied(_) => {}
        }
    }
    Ok(scoped)
}
