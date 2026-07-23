use futures_util::{StreamExt, stream};
use node_semver::{Range as SemverRange, Version};
use pacquet_config::matcher::{Matcher, create_matcher};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pacquet_resolving_git_resolver::{GitCommandRunner, RealGitRunner, get_repo_refs};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
};
use tokio::fs;
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

pub fn is_selector(selector: &str) -> bool {
    let pattern = selector.strip_prefix('!').unwrap_or(selector);
    !pattern.starts_with('@') && pattern.contains('/')
}

pub fn normalize_selector(selector: &str) -> String {
    if !is_selector(selector) {
        return selector.to_string();
    }
    selector.rsplit_once('@').map_or(selector, |(name, _)| name).to_string()
}

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
        &resolve_server_url(server_url),
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
        &resolve_server_url(server_url),
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
    let updates = plans
        .into_iter()
        .filter(|plan| {
            let target = if latest { &plan.latest } else { &plan.wanted };
            plan.current.version <= target.version
                && (plan.action.ref_ != target.commit
                    || plan.action.comment_version.as_deref() != Some(&target.tag))
        })
        .collect::<Vec<_>>();
    let mut edits: BTreeMap<PathBuf, Vec<(Range<usize>, String)>> = BTreeMap::new();
    for plan in &updates {
        let target = if latest { &plan.latest } else { &plan.wanted };
        edits
            .entry(plan.action.file.clone())
            .or_default()
            .push((plan.action.range.clone(), render_target_value(&plan.action, target)));
    }
    for (file, mut replacements) in edits {
        let file_display = file.display().to_string();
        let mut text = fs::read_to_string(&file)
            .await
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        replacements.sort_by_key(|(range, _)| Reverse(range.start));
        for (range, new) in replacements {
            text.replace_range(range, &new);
        }
        tokio::task::spawn_blocking(move || pacquet_fs::write_atomic(&file, text.as_bytes()))
            .await
            .map_err(|error| miette::miette!("Failed to write {file_display}: {error}"))?
            .map_err(|error| miette::miette!("Failed to write {file_display}: {error}"))?;
    }
    Ok(to_outdated(updates, latest, server_url))
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
    let refs_by_repo = stream::iter(repos)
        .map(|repo| async move {
            let url = format!("{server_url}/{repo}.git");
            match get_repo_refs(runner, &url, None).await {
                Ok(refs) => {
                    let versions = repo_versions(&refs);
                    Some((repo, versions))
                }
                Err(error) => {
                    global_warn::<Reporter>(format!(
                        r#"Skipping the GitHub Actions from "{repo}": {error}"#,
                    ));
                    None
                }
            }
        })
        .buffer_unordered(GIT_CONCURRENCY)
        .filter_map(|entry| async move { entry })
        .collect::<HashMap<_, _>>()
        .await;
    let mut plans = Vec::new();
    for action in actions {
        let Some(versions) = refs_by_repo.get(&action.repo) else { continue };
        let Some(current) = find_current(&action, versions) else { continue };
        let wanted_range =
            SemverRange::parse(format!("^{}", current.version)).map_err(|error| {
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
        let Some(latest) = candidates.last() else { continue };
        let Some(wanted) =
            candidates.iter().rev().find(|candidate| wanted_range.satisfies(&candidate.version))
        else {
            continue;
        };
        plans.push(PlannedUpdate {
            action,
            current,
            latest: (*latest).clone(),
            wanted: (*wanted).clone(),
        });
    }
    Ok(plans)
}

async fn discover(root: &Path) -> miette::Result<Vec<ActionReference>> {
    let root_display = root.display();
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(|error| miette::miette!("Failed to read {root_display}: {error}"))?;
    let workflows = root.join(".github/workflows");
    let workflows_display = workflows.display();
    let mut entries = match fs::read_dir(&workflows).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(miette::miette!("Failed to read {workflows_display}: {error}"));
        }
    };
    let mut queue = VecDeque::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| miette::miette!("Failed to read {workflows_display}: {error}"))?
    {
        let path = entry.path();
        if matches!(path.extension().and_then(|ext| ext.to_str()), Some("yml" | "yaml")) {
            queue.push_back(path);
        }
    }
    let mut visited = HashSet::new();
    let mut actions = Vec::new();
    while let Some(file) = queue.pop_front() {
        let file_display = file.display();
        let real_file = fs::canonicalize(&file)
            .await
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        if !real_file.starts_with(&canonical_root) {
            return Err(miette::miette!(
                code = "ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_OUTSIDE_ROOT",
                "GitHub Actions workflow is outside the project root: {file_display}"
            ));
        }
        if !visited.insert(real_file.clone()) {
            continue;
        }
        let real_file_display = real_file.display();
        let text = fs::read_to_string(&real_file)
            .await
            .map_err(|error| miette::miette!("Failed to read {real_file_display}: {error}"))?;
        for uses_value in uses_values(&text)
            .map_err(|error| miette::miette!("Failed to parse {real_file_display}: {error}"))?
        {
            let original_value = uses_value.value;
            let (value, comment) = split_uses_value(original_value);
            if let Some(local) = value.strip_prefix("./") {
                if let Some(candidate) =
                    resolve_local_reference(root, &canonical_root, local).await?
                {
                    queue.push_back(candidate);
                }
                continue;
            }
            let Some((name, ref_and_comment)) = value.rsplit_once('@') else { continue };
            if name.starts_with("docker://") {
                continue;
            }
            let mut parts = name.split('/');
            let (Some(owner), Some(repository)) = (parts.next(), parts.next()) else { continue };
            let comment_version = comment
                .and_then(|comment| comment.split_whitespace().next())
                .filter(|candidate| parse_version(candidate).is_some())
                .map(str::to_string);
            actions.push(ActionReference {
                comment_version,
                file: real_file.clone(),
                flow_style: uses_value.flow_style,
                indentation: uses_value.indentation,
                name: name.to_string(),
                original_value: original_value.to_string(),
                range: uses_value.range,
                ref_: ref_and_comment.to_string(),
                repo: format!("{owner}/{repository}"),
            });
        }
    }
    Ok(actions)
}

async fn resolve_local_reference(
    root: &Path,
    canonical_root: &Path,
    reference: &str,
) -> miette::Result<Option<PathBuf>> {
    let target = root.join(reference);
    let candidate = if matches!(
        target.extension().and_then(|extension| extension.to_str()),
        Some("yml" | "yaml"),
    ) {
        existing_file(&target).await?.then_some(target)
    } else {
        let action_yml = target.join("action.yml");
        if existing_file(&action_yml).await? {
            Some(action_yml)
        } else {
            let action_yaml = target.join("action.yaml");
            existing_file(&action_yaml).await?.then_some(action_yaml)
        }
    };
    let Some(candidate) = candidate else { return Ok(None) };
    let candidate_display = candidate.display();
    let candidate = fs::canonicalize(&candidate)
        .await
        .map_err(|error| miette::miette!("Failed to read {candidate_display}: {error}"))?;
    Ok(candidate.starts_with(canonical_root).then_some(candidate))
}

async fn existing_file(path: &Path) -> miette::Result<bool> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            let path_display = path.display();
            Err(miette::miette!("Failed to read {path_display}: {error}"))
        }
    }
}

fn split_uses_value(value: &str) -> (&str, Option<&str>) {
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|quote| matches!(quote, '\'' | '"'))
        && let Some(end) = value[1..].find(quote)
    {
        let end = end + 1;
        let comment = value[end + quote.len_utf8()..].trim().strip_prefix('#').map(str::trim);
        return (&value[1..end], comment);
    }
    value.split_once(" #").map_or((value, None), |(value, comment)| (value, Some(comment.trim())))
}

struct UsesValue<'a> {
    flow_style: bool,
    indentation: String,
    range: Range<usize>,
    value: &'a str,
}

fn uses_values(text: &str) -> Result<Vec<UsesValue<'_>>, QueryError> {
    let value = yaml_serde::from_str::<Value>(text).map_err(|_| QueryError::InvalidInput)?;
    let document = Document::new(text)?;
    let mut values = Vec::new();
    for route in uses_routes(&value) {
        let Some(feature) = document.query_exact(&route)? else {
            continue;
        };
        let key = document.query_key_only(&route)?;
        let (start, scalar_end) = feature.location.byte_span;
        let separator = start
            .checked_sub(key.location.byte_span.1)
            .map(|_| &text[key.location.byte_span.1..start]);
        if separator.is_none_or(|separator| {
            !separator.starts_with(':') || !separator[1..].chars().all(char::is_whitespace)
        }) {
            continue;
        }
        let line_end = text[scalar_end..].find('\n').map_or(text.len(), |end| scalar_end + end);
        let trailing = &text[scalar_end..line_end];
        let following = trailing.trim_start();
        let flow_style = matches!(following.chars().next(), Some('}' | ']' | ','));
        let end = if following.starts_with('#') {
            scalar_end + trailing.trim_end().len()
        } else if flow_style {
            scalar_end + trailing.len() - following.len()
        } else {
            scalar_end
        };
        let line_start = text[..start].rfind('\n').map_or(0, |line_break| line_break + 1);
        values.push(UsesValue {
            flow_style,
            indentation: " ".repeat(start - line_start),
            range: start..end,
            value: &text[start..end],
        });
    }
    Ok(values)
}

fn uses_routes(value: &Value) -> Vec<Route<'static>> {
    let mut routes = Vec::new();
    if let Some(jobs) = value.get("jobs").and_then(Value::as_mapping) {
        for (name, job) in jobs {
            let Some(name) = name.as_str() else { continue };
            if job.get("uses").and_then(Value::as_str).is_some() {
                routes.push(Route::from(vec![
                    "jobs".into(),
                    name.to_string().into(),
                    "uses".into(),
                ]));
            }
            add_step_routes(
                &mut routes,
                job.get("steps"),
                &["jobs".into(), name.to_string().into(), "steps".into()],
            );
        }
    }
    add_step_routes(
        &mut routes,
        value.get("runs").and_then(|runs| runs.get("steps")),
        &["runs".into(), "steps".into()],
    );
    routes
}

fn add_step_routes(
    routes: &mut Vec<Route<'static>>,
    steps: Option<&Value>,
    prefix: &[Component<'static>],
) {
    let Some(steps) = steps.and_then(Value::as_sequence) else { return };
    for (index, step) in steps.iter().enumerate() {
        if step.get("uses").and_then(Value::as_str).is_none() {
            continue;
        }
        let mut route = prefix.to_owned();
        route.extend([index.into(), "uses".into()]);
        routes.push(Route::from(route));
    }
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
                homepage: format!("{server_url}/{}", plan.action.repo),
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
fn resolve_server_url(server_url: Option<&str>) -> String {
    server_url
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("GITHUB_SERVER_URL").ok().filter(|url| !url.is_empty()))
        .unwrap_or_else(|| "https://github.com".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn global_warn<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

#[cfg(test)]
mod tests;
