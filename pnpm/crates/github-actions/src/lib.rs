//! Inspect GitHub Actions dependencies and update their commit pins while preserving workflow formatting.

use edits::{apply_workflow_edits, planned_edits};
use futures_util::{StreamExt, stream};
use node_semver::{Range as SemverRange, Version};
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::{redact_and_sanitize, redact_url_for_display};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_resolving_git_resolver::{GitCommandRunner, RealGitRunner, get_repo_refs};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
};
use tokio::fs;
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

#[must_use]
pub fn is_selector(selector: &str) -> bool {
    let pattern = selector.strip_prefix('!').unwrap_or(selector);
    !pattern.starts_with('@') && pattern.contains('/')
}

#[must_use]
pub fn normalize_selector(selector: &str) -> String {
    if !is_selector(selector) {
        return selector.to_string();
    }
    selector.rsplit_once('@').map_or(selector, |(name, _)| name).to_string()
}

#[must_use]
pub fn selector_matcher(selectors: &[String]) -> Option<Matcher> {
    if selectors.is_empty() {
        return None;
    }
    Some(create_matcher(
        &selectors.iter().map(|selector| normalize_selector(selector)).collect::<Vec<_>>(),
    ))
}

#[derive(Clone)]
struct ActionReference {
    comment_version: Option<String>,
    file: PathBuf,
    flow_style: bool,
    indentation: String,
    name: String,
    original_value: String,
    range: Range<usize>,
    ref_: String,
    repo: String,
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
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    find_outdated_with_runner::<Reporter, _>(
        root,
        compatible,
        matcher,
        &resolve_server_url(server_url)?,
        &RealGitRunner::new(),
    )
    .await
}

async fn find_outdated_with_runner<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    root: &Path,
    compatible: bool,
    matcher: Option<&Matcher>,
    server_url: &str,
    runner: &Runner,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan::<Reporter, _>(root, matcher, server_url, runner).await?;
    Ok(to_outdated(plans, !compatible, server_url))
}

pub async fn update<Reporter: self::Reporter>(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
    server_url: Option<&str>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    update_with_runner::<Reporter, _>(
        root,
        latest,
        matcher,
        &resolve_server_url(server_url)?,
        &RealGitRunner::new(),
    )
    .await
}

async fn update_with_runner<Reporter: self::Reporter, Runner: GitCommandRunner + Sync>(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
    server_url: &str,
    runner: &Runner,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan::<Reporter, _>(root, matcher, server_url, runner).await?;
    let updates =
        plans.into_iter().filter(|plan| plan_is_outdated(plan, latest)).collect::<Vec<_>>();
    apply_workflow_edits(planned_edits(&updates, latest)).await?;
    Ok(to_outdated(updates, latest, server_url))
}

/// Whether the workflow's pin still differs from the version it would move
/// to. A pin ahead of the target is left alone.
fn plan_is_outdated(plan: &PlannedUpdate, latest: bool) -> bool {
    let target = update_target(plan, latest);
    plan.current.version <= target.version
        && (plan.action.ref_ != target.commit
            || plan.action.comment_version.as_deref() != Some(&target.tag))
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
) -> miette::Result<Vec<PlannedUpdate>> {
    let actions = discover(root)
        .await?
        .into_iter()
        .filter(|action| {
            matcher.is_none_or(|matcher| {
                matcher.matches(&action.name) || matcher.matches(&action.repo)
            })
        })
        .collect::<Vec<_>>();
    let repos = actions.iter().map(|action| action.repo.clone()).collect::<BTreeSet<_>>();
    let refs_by_repo = versions_by_repo::<Reporter, Runner>(repos, server_url, runner).await;
    let mut plans = Vec::new();
    for action in actions {
        let Some(versions) = refs_by_repo.get(&action.repo) else { continue };
        if let Some(plan) = plan_action_update(action, versions)? {
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
) -> miette::Result<Option<PlannedUpdate>> {
    let Some(current) = find_current(&action, versions) else { return Ok(None) };
    let wanted_range = SemverRange::parse(format!("^{}", current.version)).map_err(|error| {
        miette::miette!(
            "Failed to create a compatible GitHub Action range for {}: {error}",
            current.version,
        )
    })?;
    let candidates = versions
        .iter()
        .filter(|candidate| {
            !current.version.pre_release.is_empty() || candidate.version.pre_release.is_empty()
        })
        .collect::<Vec<_>>();
    let Some(latest) = candidates.last() else { return Ok(None) };
    let Some(wanted) =
        candidates.iter().rev().find(|candidate| wanted_range.satisfies(&candidate.version))
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

fn repo_versions(refs: &HashMap<String, String>) -> Vec<RepoVersion> {
    let mut versions = refs
        .iter()
        .filter_map(|(ref_, commit)| {
            let tag = ref_.strip_prefix("refs/tags/")?;
            if tag.ends_with("^{}") {
                return None;
            }
            let version = parse_version(tag)?;
            Some(RepoVersion {
                commit: refs.get(&format!("{ref_}^{{}}")).unwrap_or(commit).clone(),
                tag: tag.to_string(),
                version,
            })
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| left.version.cmp(&right.version));
    versions
}

fn find_current(action: &ActionReference, versions: &[RepoVersion]) -> Option<RepoVersion> {
    if is_sha(&action.ref_)
        && let Some(comment) = &action.comment_version
        && let Some(version) = parse_version(comment)
        && let Some(current) = versions
            .iter()
            .find(|candidate| candidate.commit == action.ref_ && candidate.version == version)
    {
        return Some(current.clone());
    }
    if let Some(version) = parse_version(&action.ref_) {
        return versions.iter().find(|candidate| candidate.version == version).cloned();
    }
    if let Ok(major) = action.ref_.trim_start_matches('v').parse::<u64>() {
        return versions
            .iter()
            .rfind(|candidate| {
                candidate.version.major == major && candidate.version.pre_release.is_empty()
            })
            .cloned();
    }
    if is_sha(&action.ref_) {
        return versions.iter().rfind(|candidate| candidate.commit == action.ref_).cloned();
    }
    None
}

fn render_target_ref(target: &RepoVersion) -> String {
    target.commit.clone()
}

fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn render_target_value(action: &ActionReference, target: &RepoVersion) -> String {
    let old_reference = format!("{}@{}", action.name, action.ref_);
    let new_reference = format!("{}@{}", action.name, render_target_ref(target));
    let mut value = action.original_value.replacen(&old_reference, &new_reference, 1);
    if let Some(comment_version) = &action.comment_version {
        value = value.replacen(comment_version, &target.tag, 1);
    } else if let Some(comment) = value.find(" #") {
        value.insert_str(comment + 2, &format!("{} ", target.tag));
    } else if action.flow_style {
        value.truncate(value.trim_end().len());
        value.push_str(" # ");
        value.push_str(&target.tag);
        value.push('\n');
        value.push_str(&action.indentation);
    } else {
        value.push_str(" # ");
        value.push_str(&target.tag);
    }
    value
}

fn parse_version(input: &str) -> Option<Version> {
    Version::parse(input).or_else(|_| Version::parse(input.trim_start_matches('v'))).ok()
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
            plan.action.name.clone(),
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
        .or_else(|| std::env::var("GITHUB_SERVER_URL").ok().filter(|url| !url.is_empty()))
        .unwrap_or_else(|| "https://github.com".to_string());
    validate_server_url(&url)
}

fn validate_server_url(url: &str) -> miette::Result<String> {
    let parsed = url::Url::parse(url).ok().filter(|parsed| {
        parsed.host_str().is_some() && pnpm_network::is_url_secure_for_credentials(parsed.as_str())
    });
    let Some(parsed) = parsed else {
        return Err(miette::miette!(
            code = "ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL",
            "The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts",
        ));
    };
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

fn global_warn<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

#[cfg(test)]
mod tests;

mod edits;
mod workflow;
