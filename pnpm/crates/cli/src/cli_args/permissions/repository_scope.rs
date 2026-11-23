use super::{
    Capabilities,
    capability_list,
    disambiguated_labels,
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
    BTreeSet,
};

#[derive(Default)]
struct PackageGroup {
    keys: Vec<String>,
    requests: Capabilities,
    git: bool,
}

pub(super) fn prompt_for_packages(
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<BTreeSet<String>>> {
    let message = "Choose which packages to approve";
    let groups = package_groups(pending);
    if !groups.values().any(|group| group.git) {
        return Ok(prompt_for_choices(message, &package_choices(pending))?.map(BTreeSet::from_iter));
    }
    let prompt = grouped_prompt(message, &groups);
    Ok(match prompt.interact().into_diagnostic()? {
        CheckboxAnswer::Selected(selected) => Some(selected.into_iter().flatten().collect()),
        CheckboxAnswer::Cancelled => None,
    })
}

fn package_groups(pending: &BTreeMap<String, Capabilities>) -> BTreeMap<String, PackageGroup> {
    let mut groups = BTreeMap::<String, PackageGroup>::new();
    for (key, requests) in pending {
        let repo = git_repo_allow_build_key_from_dep_path(key);
        let git = repo.is_some();
        let identity = repo.map(std::borrow::Cow::into_owned).unwrap_or_else(|| key.clone());
        let group = groups
            .entry(identity)
            .or_insert_with(|| PackageGroup { git, ..Default::default() });
        group.keys.push(key.clone());
        group.requests.extend(requests);
    }
    groups
}

fn grouped_prompt(
    message: &str,
    groups: &BTreeMap<String, PackageGroup>,
) -> CheckboxPrompt<Vec<String>> {
    let labels = disambiguated_labels(groups.keys().map(String::as_str));
    let mut items = Vec::new();
    let mut alternatives = BTreeMap::new();
    for (key, group) in groups {
        let label = format!("{}  {}", labels[key.as_str()], capability_list(&group.requests));
        let mut choice = CheckboxChoice {
            short: label.clone(),
            name: label.clone(),
            description: None,
            value: group.keys.clone(),
        };
        if group.git {
            let count = group.keys.len();
            let scope = if count == 1 {
                "this commit".to_string()
            } else {
                format!("{count} pending commits")
            };
            choice.name = format!("{label}  [{scope}]");
            choice.description = Some("r: include future commits".to_string());
            alternatives.insert(
                items.len(),
                CheckboxChoice {
                    name: format!("{label}  [all commits of {key}]"),
                    short: format!("{label} (all commits)"),
                    description: Some("r: pending commits only".to_string()),
                    value: vec![key.clone()],
                },
            );
        }
        items.push(CheckboxItem::Choice(choice));
    }
    CheckboxPrompt::new(message, items).with_alternatives(alternatives)
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

#[cfg(test)]
mod tests;
