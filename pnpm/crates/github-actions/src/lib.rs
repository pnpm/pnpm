//! Inspect GitHub Actions dependencies and update their commit pins while preserving workflow formatting.

pub(crate) use pnpm_process as process;

pub use release_age::ReleaseAge;
pub use selectors::{is_selector, normalize_selector, selector_matcher};

use edits::{apply_workflow_edits, planned_edits};
use futures_util::{StreamExt, stream};
use node_semver::{Range as SemverRange, Version};
use pnpm_matcher::Matcher;
use pnpm_network::{redact_and_sanitize, redact_url_for_display};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_resolving_git_resolver::{GitCommandRunner, RealGitRunner, get_repo_refs};
use release_age::{GitTagDates, ReleaseAgeCheck, ReleaseDates};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
};
use tokio::fs;
use versions::{find_current, parse_comment_version, repo_versions};
use workflow::discover;
use yaml_serde::Value;
use yamlpath::{Component, Document, QueryError, Route};

#[derive(Clone)]
pub struct OutdatedGitHubAction {
    pub current: Version,
    pub homepage: String,
    pub latest: Version,
    pub name: String,
    pub wanted: Version,
}

#[derive(Clone)]
struct ActionReference {
    file: PathBuf,
    name: String,
    ref_: String,
    repo: String,
    source: WorkflowValue,
}

#[derive(Clone)]
struct WorkflowValue {
    comment_version: Option<String>,
    flow_style: bool,
    indentation: String,
    original_value: String,
    range: Range<usize>,
}

#[derive(Clone)]
struct RepoVersion {
    commit: String,
    tag: String,
    version: Version,
}

struct PlannedUpdate {
    action: ActionReference,
    current: RepoVersion,
    latest: RepoVersion,
    wanted: RepoVersion,
}

const GIT_CONCURRENCY: usize = 8;

pub async fn find_outdated<Reporter: self::Reporter>(
    root: &Path,
    compatible: bool,
    matcher: Option<&Matcher>,
    server_url: Option<&str>,
    release_age: Option<&ReleaseAge>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    find_outdated_with_runner::<Reporter, _>(
        root,
        compatible,
        matcher,
        &resolve_server_url(server_url)?,
        &RealGitRunner::new(),
        release_age.map(|policy| ReleaseAgeCheck { policy, dates: &GitTagDates }),
    )
    .await
}

async fn find_outdated_with_runner<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    root: &Path,
    compatible: bool,
    matcher: Option<&Matcher>,
    server_url: &str,
    runner: &Runner,
    release_age: Option<ReleaseAgeCheck<'_>>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan::<Reporter, _>(root, matcher, server_url, runner, release_age).await?;
    Ok(to_outdated(plans, !compatible, server_url))
}

pub async fn update<Reporter: self::Reporter>(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
    server_url: Option<&str>,
    release_age: Option<&ReleaseAge>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    update_with_runner::<Reporter, _>(
        root,
        latest,
        matcher,
        &resolve_server_url(server_url)?,
        &RealGitRunner::new(),
        release_age.map(|policy| ReleaseAgeCheck { policy, dates: &GitTagDates }),
    )
    .await
}

async fn update_with_runner<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
    server_url: &str,
    runner: &Runner,
    release_age: Option<ReleaseAgeCheck<'_>>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan::<Reporter, _>(root, matcher, server_url, runner, release_age).await?;
    let updates = plans
        .into_iter()
        .filter(|plan| plan_is_outdated(plan, latest))
        .collect::<Vec<_>>();
    apply_workflow_edits(planned_edits(&updates, latest)).await?;
    Ok(to_outdated(updates, latest, server_url))
}

/// Whether the workflow's pin still differs from the version it would move
/// to. A pin ahead of the target is left alone.
fn plan_is_outdated(plan: &PlannedUpdate, latest: bool) -> bool {
    let target = update_target(plan, latest);
    plan.current.version <= target.version
        && (plan.action.ref_ != target.commit
            || plan.action.source.comment_version.as_deref() != Some(&target.tag))
}

/// The version this update moves to: the newest release under `latest`,
/// otherwise the newest within the range the workflow declares.
fn update_target(plan: &PlannedUpdate, latest: bool) -> &RepoVersion {
    if latest { &plan.latest } else { &plan.wanted }
}

async fn create_plan<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    root: &Path,
    matcher: Option<&Matcher>,
    server_url: &str,
    runner: &Runner,
    release_age: Option<ReleaseAgeCheck<'_>>,
) -> miette::Result<Vec<PlannedUpdate>> {
    let actions = discover(root).await?
        .into_iter()
        .filter(|action| matcher.is_none_or(|matcher| matcher.matches(&action.name)))
        .collect::<Vec<_>>();
    let repos = actions
        .iter()
        .map(|action| action.repo.clone())
        .collect::<BTreeSet<_>>();
    let mut refs_by_repo = versions_by_repo::<Reporter, Runner>(repos, server_url, runner).await;
    let release_dates =
        ReleaseDates::read::<Reporter>(release_age, &actions, &mut refs_by_repo, server_url).await;
    let mut plans = Vec::new();
    for action in actions {
        let Some(versions) = refs_by_repo.get(&action.repo) else { continue };
        if let Some(plan) = plan_action_update(action, versions, &release_dates)? {
            plans.push(plan);
        }
    }
    Ok(plans)
}

/// Each repository's tagged versions, skipping (with a warning) the ones
/// whose refs cannot be listed.
async fn versions_by_repo<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    repos: BTreeSet<String>,
    server_url: &str,
    runner: &Runner,
) -> HashMap<String, Vec<RepoVersion>> {
    stream::iter(repos)
        .map(|repo| async move {
            let url = format!("{server_url}/{repo}.git");
            match get_repo_refs(runner, &url, None).await {
                Ok(refs) => {
                    let versions = repo_versions(&refs);
                    Some((repo, versions))
                }
                Err(error) => {
                    // The git error may echo a credentialed URL or raw stderr
                    // back, so it is redacted and stripped of control
                    // characters before logging.
                    global_warn::<Reporter>(redact_and_sanitize(&format!(
                        r#"Skipping the GitHub Actions from "{repo}": {error}"#,
                    )));
                    None
                }
            }
        })
        .buffer_unordered(GIT_CONCURRENCY)
        .filter_map(|entry| async move { entry })
        .collect::<HashMap<_, _>>()
        .await
}

/// The update for one action reference: its current version, the newest
/// compatible one and the latest, or `None` when they cannot be told.
fn plan_action_update(
    action: ActionReference,
    versions: &[RepoVersion],
    release_dates: &ReleaseDates<'_>,
) -> miette::Result<Option<PlannedUpdate>> {
    let Some(current) = find_current(&action, versions) else { return Ok(None) };
    let wanted_range = SemverRange::parse(format!("^{}", current.version))
        .map_err(|error| {
            miette::miette!(
                "Failed to create a compatible GitHub Action range for {}: {error}",
                current.version,
            )
        })?;
    let candidates = versions
        .iter()
        .filter(|candidate| {
            (!current.version.pre_release.is_empty()
                || candidate.version.pre_release.is_empty())
                && (candidate.version <= current.version
                    || release_dates.admits(&action, candidate))
        })
        .collect::<Vec<_>>();
    let Some(latest) = candidates.last() else { return Ok(None) };
    let Some(wanted) = candidates
        .iter()
        .rev()
        .find(|candidate| wanted_range.satisfies(&candidate.version))
    else {
        return Ok(None);
    };
    Ok(Some(PlannedUpdate {
        action,
        current,
        latest: (*latest).clone(),
        wanted: (*wanted).clone(),
    }))
}

fn to_outdated(
    plans: Vec<PlannedUpdate>,
    latest: bool,
    server_url: &str,
) -> Vec<OutdatedGitHubAction> {
    let mut actions = BTreeMap::new();
    for plan in plans {
        let target = if latest { plan.latest } else { plan.wanted.clone() };
        if plan.current.version >= target.version {
            continue;
        }
        actions.insert(
            (plan.action.name.clone(), plan.current.version.clone()),
            OutdatedGitHubAction {
                current: plan.current.version,
                homepage: redact_url_for_display(&format!("{server_url}/{}", plan.action.repo)),
                latest: target.version,
                name: plan.action.name,
                wanted: plan.wanted.version,
            },
        );
    }
    actions.into_values().collect()
}

/// Resolves the effective GitHub server base URL: the
/// `update.githubActionsServer` setting, the `GITHUB_SERVER_URL`
/// environment variable, or <https://github.com> — first non-empty wins.
fn resolve_server_url(server_url: Option<&str>) -> miette::Result<String> {
    let url = server_url
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("GITHUB_SERVER_URL")
                .ok()
                .filter(|url| !url.is_empty())
        })
        .unwrap_or_else(|| "https://github.com".to_string());
    validate_server_url(&url)
}

fn validate_server_url(url: &str) -> miette::Result<String> {
    let parsed = url::Url::parse(url)
        .ok()
        .filter(|parsed| {
            parsed.host_str().is_some()
                && pnpm_network::is_url_secure_for_credentials(parsed.as_str())
        });
    let Some(parsed) = parsed else {
        return Err(miette::miette!(
            code = "ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL",
            "The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts",
        ));
    };
    Ok(parsed
        .as_str()
        .trim_end_matches('/')
        .to_string())
}

fn global_warn<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

#[cfg(test)]
mod tests;

mod edits;
mod release_age;
mod selectors;
mod versions;
mod workflow;
