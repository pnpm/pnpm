use super::{
    Capabilities,
    package_choices,
    prompt_for_choices,
};
use crate::checkbox_prompt::{
    CheckboxAlternative,
    CheckboxAnswer,
    CheckboxChoice,
    CheckboxItem,
    CheckboxPrompt,
};
use miette::IntoDiagnostic;
use pnpm_package_name::git_repo_allow_build_key_from_dep_path;
use std::collections::{
    BTreeMap,
    BTreeSet,
};

pub(super) fn prompt_for_packages(
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<BTreeSet<String>>> {
    let choices = package_choices(pending);
    let alternatives = repository_alternatives(&choices);
    let message = "Choose which packages to approve";
    if alternatives.is_empty() {
        return Ok(prompt_for_choices(message, &choices)?.map(BTreeSet::from_iter));
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
                value: key,
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
) -> BTreeMap<usize, CheckboxAlternative<String>> {
    let mut groups = BTreeMap::new();
    choices
        .iter()
        .enumerate()
        .filter_map(|(index, (key, label))| {
            let repo = git_repo_allow_build_key_from_dep_path(key)?.into_owned();
            let group = *groups.entry(repo.clone()).or_insert(index);
            let choice = CheckboxChoice {
                name: format!("{label}  [all commits of {repo}]"),
                short: format!("{label} (all commits)"),
                value: repo,
                description: Some("r: switch to this commit".to_string()),
            };
            Some((index, CheckboxAlternative::new(group, choice)))
        })
        .collect()
}

/// Combine the requests covered by each selected repository. Every other key
/// keeps its commit-specific requests.
pub(super) fn group_requests<Request: Ord>(
    requests: BTreeMap<String, impl IntoIterator<Item = Request>>,
    selected: &BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<Request>> {
    let mut grouped = BTreeMap::<String, BTreeSet<Request>>::new();
    for (key, values) in requests {
        let repo = git_repo_allow_build_key_from_dep_path(&key)
            .filter(|repo| selected.contains(repo.as_ref()))
            .map(std::borrow::Cow::into_owned);
        grouped
            .entry(repo.unwrap_or(key))
            .or_default()
            .extend(values);
    }
    grouped
}
