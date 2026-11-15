#[cfg(not(target_family = "wasm"))]
use dialoguer::{Confirm, MultiSelect};
#[cfg(not(target_family = "wasm"))]
use miette::IntoDiagnostic;
use pnpm_package_manager::short_package_label;
use std::collections::{BTreeMap, BTreeSet};

/// Given package keys, format each into a short label, appending
/// `[{key}]` if the short label collides with another key's. A key given
/// twice is one key.
pub(crate) fn disambiguated_labels<'a>(
    keys: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<&'a str, String> {
    let items: BTreeSet<&'a str> = keys.into_iter().collect();
    let mut counts = BTreeMap::<String, usize>::new();
    for &key in &items {
        *counts
            .entry(short_package_label(key))
            .or_default() += 1;
    }
    items
        .into_iter()
        .map(|key| {
            let base = short_package_label(key);
            let label = if counts.get(&base).copied().unwrap_or(0) > 1 {
                format!("{base} [{key}]")
            } else {
                base
            };
            (key, label)
        })
        .collect()
}

/// Turn `keys` into `(key, label)` choices for a checkbox prompt,
/// keeping choices with colliding short labels distinguishable by
/// appending their approval key.
pub(super) fn build_choices(keys: &[String]) -> Vec<(String, String)> {
    let labels = disambiguated_labels(keys.iter().map(String::as_str));
    keys.iter()
        .map(|key| (key.clone(), labels[key.as_str()].clone()))
        .collect()
}

/// Show the checkbox prompt and return the chosen package names, or `None`
/// when the prompt is interrupted.
pub(super) fn prompt_for_builds(
    automatically_ignored_builds: &[String],
) -> miette::Result<Option<Vec<String>>> {
    let choices = build_choices(&super::sort_unique(automatically_ignored_builds.to_vec()));
    super::prompt_for_choices("Choose which packages to build", &choices)
}

/// Show a checkbox prompt over `choices` (value, label) and return the
/// chosen values, or `None` when the prompt is interrupted.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn prompt_for_choices(
    prompt: &str,
    choices: &[(String, String)],
) -> miette::Result<Option<Vec<String>>> {
    let labels: Vec<&str> = choices
        .iter()
        .map(|(_, label)| label.as_str())
        .collect();
    match MultiSelect::new()
        .with_prompt(format!("{prompt} (<space> to select, <enter> to confirm)"))
        .items(&labels)
        .interact_opt()
        .into_diagnostic()?
    {
        Some(indices) => Ok(Some(
            indices
                .into_iter()
                .map(|index| choices[index].0.clone())
                .collect(),
        )),
        None => Ok(None),
    }
}

/// The confirmation prompt text for building `build_packages`, labelled
/// as the checkbox prompt over `pending` labelled them.
pub(super) fn confirm_builds_prompt(build_packages: &[String], pending: &[String]) -> String {
    let labels = disambiguated_labels(pending.iter().map(String::as_str));
    format!(
        "Build {}?",
        build_packages
            .iter()
            .map(|key| {
                labels
                    .get(key.as_str())
                    .cloned()
                    .unwrap_or_else(|| short_package_label(key))
            })
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Ask a yes/no question that defaults to "no", matching pnpm's
/// `confirm({ default: false })`.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn confirm(message: &str) -> miette::Result<bool> {
    Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .into_diagnostic()
}
