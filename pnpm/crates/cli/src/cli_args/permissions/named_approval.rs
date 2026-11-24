use super::{
    Approval,
    Capabilities,
    PermissionCapability,
    decisions::{
        Decisions,
        PackageDecisions,
    },
    partition_params,
    prompt_for_choices,
};
use crate::{
    cli_args::approve_builds::ApproveBuildsError,
    confirm_prompt::PromptAvailability,
};
use indexmap::IndexSet;
use pnpm_package_name::{
    git_repo_allow_build_key_from_dep_path,
    is_git_repo_dep_path,
    parse_dep_path_name_version,
};
use pnpm_reporter::{
    Reporter,
    emit_global_warning,
};
use std::collections::{
    BTreeMap,
    BTreeSet,
};

const ALL_CAPABILITIES: [PermissionCapability; 2] =
    [PermissionCapability::Build, PermissionCapability::Skills];

pub(super) fn named_approval<Reporter: self::Reporter>(
    params: &[String],
    pending: &BTreeMap<String, Capabilities>,
    prompt: PromptAvailability,
) -> miette::Result<Option<Approval>> {
    let requests = requests_by_selector(pending);
    let pending_names = requests
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let partition = partition_params(params, &pending_names);
    partition.reject_contradictions()?;
    if !partition.unknown.is_empty() {
        validate_git_selectors(&partition.unknown, pending)?;
        emit_global_warning::<Reporter>(&format!(
            "The following packages are not awaiting approval: {}",
            partition.unknown.join(", "),
        ));
    }
    let mut decisions = Decisions::new();
    for pkg in IndexSet::<_>::from_iter(partition.approved) {
        if let Some(requested) = requests.get(&pkg) {
            decisions.insert(pkg, PackageDecisions::for_capabilities(requested, true));
            continue;
        }
        let Some(selected) = choose_capabilities(&pkg, prompt)? else {
            return Ok(None);
        };
        if !selected.is_empty() {
            decisions.insert(pkg, PackageDecisions::for_capabilities(&selected, true));
        }
    }
    let all = ALL_CAPABILITIES.into_iter().collect();
    for pkg in partition.denied {
        let decided = PackageDecisions::for_capabilities(requests.get(&pkg).unwrap_or(&all), false);
        decisions.insert(pkg, decided);
    }
    Ok((!decisions.is_empty()).then_some(Approval { decisions, skills_dirs: Vec::new() }))
}

fn choose_capabilities(
    pkg: &str,
    prompt: PromptAvailability,
) -> miette::Result<Option<Capabilities>> {
    if !prompt.is_available() {
        return Err(miette::miette!(
            code = "ERR_PNPM_APPROVE_NON_INTERACTIVE",
            help = "Run pnpm approve in an interactive terminal outside CI, use pnpm approve-builds for a build decision, or configure permissions explicitly.",
            "Choosing permissions for {pkg} before it requests any requires an interactive terminal"
        ));
    }
    let choices = ALL_CAPABILITIES.map(|capability| {
        let label = match capability {
            PermissionCapability::Build => "Run build scripts",
            PermissionCapability::Skills => "Provide agent skills",
        };
        (capability.as_str().to_owned(), label.to_owned())
    });
    let message = format!("Which permissions do you want to grant {pkg} in advance?");
    Ok(prompt_for_choices(&message, &choices)?
        .map(|selected| {
            ALL_CAPABILITIES
                .into_iter()
                .filter(|capability| {
                    selected
                        .iter()
                        .any(|key| key == capability.as_str())
                })
                .collect()
        }))
}

fn validate_git_selectors(
    unknown: &[String],
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<()> {
    let candidates: BTreeSet<_> = unknown
        .iter()
        .map(|key| match key.split_once('#') {
            Some((repo, _)) if is_git_repo_dep_path(repo) => repo,
            _ => key.as_str(),
        })
        .collect();
    let mut suggestions = BTreeSet::new();
    for key in pending.keys() {
        let Some(repo) = git_repo_allow_build_key_from_dep_path(key) else { continue };
        let bare_name = parse_dep_path_name_version(key).map(|(name, _)| name);
        if candidates.contains(repo.as_ref())
            || bare_name.is_some_and(|name| candidates.contains(name))
        {
            suggestions.insert(key.clone());
            suggestions.insert(repo.into_owned());
        }
    }
    if suggestions.is_empty() {
        return Ok(());
    }
    Err(ApproveBuildsError::ExactSelectorRequired(suggestions.into_iter().collect()).into())
}

fn requests_by_selector(
    pending: &BTreeMap<String, Capabilities>,
) -> BTreeMap<String, Capabilities> {
    let mut requests = pending.clone();
    for (key, capabilities) in pending {
        if let Some(repo) = git_repo_allow_build_key_from_dep_path(key) {
            requests
                .entry(repo.into_owned())
                .or_default()
                .extend(capabilities);
        }
    }
    requests
}
