use super::{
    Arc,
    Config,
    DependencyGroup,
    HashSet,
    InteractiveUpdateOptions,
    InteractiveUpdateProject,
    Lockfile,
    OutdatedPackage,
    OutdatedQuery,
    Path,
    Reporter,
    TargetVersion,
    ThrottledClient,
    UpdateDetails,
    ignored_dependencies_matcher,
};
use crate::cli_args::outdated::{
    OutdatedRun,
    collect_outdated_for_importer_in_run,
};
use pnpm_github_actions as github_actions;

pub(super) struct ChoiceFilter<'a> {
    pub(super) target_version: TargetVersion<'a>,
    pub(super) include_direct: &'a [DependencyGroup],
    pub(super) details: UpdateDetails,
    pub(super) package_selectors: &'a [String],
}

pub(super) async fn gather_choices_for_projects<Reporter: self::Reporter>(
    projects: (&[InteractiveUpdateProject<'_>], &[std::path::PathBuf]),
    root: &Path,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    options: &InteractiveUpdateOptions<'_>,
) -> miette::Result<Vec<OutdatedPackage>> {
    let mut choices = match options.package_selectors {
        Some(selectors) => {
            collect_filtered_choices(
                projects,
                lockfile,
                config,
                http_client,
                ChoiceFilter {
                    target_version: options.target_version,
                    include_direct: options.include_direct,
                    details: options.details,
                    package_selectors: selectors,
                },
            )
            .await?
        }
        None => Vec::new(),
    };
    if options.include_github_actions {
        append_github_actions::<Reporter>(
            &mut choices,
            root,
            options.latest_github_actions,
            options.action_matcher.as_ref(),
            config,
        )
        .await?;
    }
    Ok(choices)
}

async fn append_github_actions<Reporter: self::Reporter>(
    choices: &mut Vec<OutdatedPackage>,
    root: &Path,
    latest: bool,
    matcher: Option<&pnpm_matcher::Matcher>,
    config: &Config,
) -> miette::Result<()> {
    choices.extend(
        github_actions::find_outdated::<Reporter>(
            root,
            !latest,
            matcher,
            config.update_config.github_actions_server.as_deref(),
            crate::github_actions::release_age(config)?.as_ref(),
        )
        .await?
        .into_iter()
        .map(OutdatedPackage::from),
    );
    Ok(())
}

pub(super) async fn collect_filtered_choices(
    (projects, standalone_dirs): (&[InteractiveUpdateProject<'_>], &[std::path::PathBuf]),
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    filter: ChoiceFilter<'_>,
) -> miette::Result<Vec<OutdatedPackage>> {
    crate::cli_args::ecosystem_deps::validate_ecosystem_selectors(
        filter.package_selectors,
        config,
    )?;
    let ignored = ignored_dependencies_matcher(config);
    let query = OutdatedQuery {
        target_version: filter.target_version,
        include_direct: filter.include_direct,
        match_names: None,
        ignore_names: ignored.as_ref(),
        include_deprecated: false,
        full_metadata: filter.details.shows_urls(),
        apply_trust_policy: config.trust_policy == pnpm_config::TrustPolicy::NoDowngrade,
        follow_prerelease_channels: false,
    };
    let run = OutdatedRun::new(config, Arc::clone(http_client), &query)?
        .with_raw_packages(normalize_choice_selectors(filter.package_selectors));
    let mut choices = futures_util::future::join_all(
        projects
            .iter()
            .map(|project| {
                collect_outdated_for_importer_in_run(
                    project.manifest,
                    lockfile,
                    &project.importer_id,
                    &query,
                    &run,
                )
            }),
    )
    .await;
    for dir in standalone_dirs {
        let (_, outdated) =
            crate::cli_args::outdated::standalone_project_outdated(dir, &query, &run).await?;
        choices.push(Ok(outdated));
    }
    unique_choices(choices)
}

fn normalize_choice_selectors(selectors: &[String]) -> Vec<String> {
    selectors
        .iter()
        .map(|selector| {
            if crate::cli_args::ecosystem_deps::is_ecosystem_prefixed(selector) {
                selector.clone()
            } else {
                pnpm_package_manager::parse_update_param(selector).pattern
            }
        })
        .collect()
}

fn unique_choices(
    choices: Vec<miette::Result<Vec<OutdatedPackage>>>,
) -> miette::Result<Vec<OutdatedPackage>> {
    // Keyed by workspace as well, so an entry each project contributed
    // survives to [`super::choices::update_choices`], that is what lets a
    // collapsed row name every project it covers instead of the first.
    let mut unique = HashSet::new();
    let mut collected = Vec::new();
    for choices in choices {
        for choice in choices? {
            let key = (
                choice.metadata.update_prefix,
                choice.alias.clone(),
                choice.package_name.clone(),
                choice.current.to_string(),
                choice.target.to_string(),
                choice.metadata.workspace.clone(),
            );
            if unique.insert(key) {
                collected.push(choice);
            }
        }
    }
    Ok(collected)
}
