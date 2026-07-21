use node_semver::Version;
use pacquet_config::matcher::Matcher;
use pacquet_resolving_git_resolver::{GitCommandRunner, RealGitRunner};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

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

#[derive(Clone)]
struct ActionReference {
    comment_version: Option<String>,
    file: PathBuf,
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

pub async fn find_outdated(
    root: &Path,
    compatible: bool,
    matcher: Option<&Matcher>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan(root, matcher, &RealGitRunner::new()).await?;
    Ok(to_outdated(plans, compatible))
}

pub async fn update(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    update_with_runner(root, latest, matcher, &RealGitRunner::new()).await
}

async fn update_with_runner<Runner: GitCommandRunner + Sync>(
    root: &Path,
    latest: bool,
    matcher: Option<&Matcher>,
    runner: &Runner,
) -> miette::Result<Vec<OutdatedGitHubAction>> {
    let plans = create_plan(root, matcher, runner).await?;
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
        let file_display = file.display();
        let mut text = fs::read_to_string(&file)
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        replacements.sort_by_key(|(range, _)| Reverse(range.start));
        for (range, new) in replacements {
            text.replace_range(range, &new);
        }
        fs::write(&file, text)
            .map_err(|error| miette::miette!("Failed to write {file_display}: {error}"))?;
    }
    Ok(to_outdated(updates, latest))
}

async fn create_plan<Runner: GitCommandRunner + Sync>(
    root: &Path,
    matcher: Option<&Matcher>,
    runner: &Runner,
) -> miette::Result<Vec<PlannedUpdate>> {
    let actions = discover(root)?
        .into_iter()
        .filter(|action| {
            matcher.is_none_or(|matcher| {
                matcher.matches(&action.name) || matcher.matches(&action.repo)
            })
        })
        .collect::<Vec<_>>();
    let repos = actions.iter().map(|action| action.repo.clone()).collect::<BTreeSet<_>>();
    let refs_by_repo =
        futures_util::future::try_join_all(repos.into_iter().map(|repo| async move {
            let url = format!("https://github.com/{repo}.git");
            let stdout = runner.ls_remote(&url, None).await.map_err(|error| {
                miette::miette!("Failed to read GitHub Action refs for {repo}: {error}")
            })?;
            Ok::<_, miette::Report>((repo, parse_repo_versions(&stdout)))
        }))
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut plans = Vec::new();
    for action in actions {
        let versions = &refs_by_repo[&action.repo];
        let Some(current) = find_current(&action, versions) else { continue };
        let candidates = versions
            .iter()
            .filter(|candidate| {
                !current.version.pre_release.is_empty() || candidate.version.pre_release.is_empty()
            })
            .collect::<Vec<_>>();
        let Some(latest) = candidates.last() else { continue };
        let Some(wanted) = candidates
            .iter()
            .rev()
            .find(|candidate| candidate.version.major == current.version.major)
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

fn discover(root: &Path) -> miette::Result<Vec<ActionReference>> {
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let workflows = root.join(".github/workflows");
    let workflows_display = workflows.display();
    let entries = match fs::read_dir(&workflows) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(miette::miette!("Failed to read {workflows_display}: {error}"));
        }
    };
    let mut queue = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(path.extension().and_then(|ext| ext.to_str()), Some("yml" | "yaml"))
        })
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::new();
    let mut actions = Vec::new();
    while let Some(file) = queue.pop_front() {
        if !visited.insert(file.clone()) {
            continue;
        }
        let file_display = file.display();
        let text = fs::read_to_string(&file)
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        for uses_value in uses_values(&text) {
            let original_value = uses_value.value;
            let (value, comment) = split_uses_value(original_value);
            if let Some(local) = value.strip_prefix("./") {
                let target = root.join(local);
                let candidate = if matches!(
                    target.extension().and_then(|ext| ext.to_str()),
                    Some("yml" | "yaml"),
                ) && target.is_file()
                {
                    Some(target)
                } else if target.join("action.yml").is_file() {
                    Some(target.join("action.yml"))
                } else if target.join("action.yaml").is_file() {
                    Some(target.join("action.yaml"))
                } else {
                    None
                };
                if let Some(candidate) = candidate
                    && let Ok(candidate) = fs::canonicalize(candidate)
                    && candidate.starts_with(&canonical_root)
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
                file: file.clone(),
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
    range: Range<usize>,
    value: &'a str,
}

fn uses_values(text: &str) -> Vec<UsesValue<'_>> {
    let mut values = Vec::new();
    let mut offset = 0;
    let mut block_scalar_indent = None;
    let mut path = Vec::new();
    for segment in text.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        let indentation = line.len() - line.trim_start_matches(' ').len();
        let trimmed = line.trim_start();
        if let Some(block_indent) = block_scalar_indent {
            if trimmed.is_empty() || indentation > block_indent {
                offset += segment.len();
                continue;
            }
            block_scalar_indent = None;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            offset += segment.len();
            continue;
        }
        while path.last().is_some_and(|(parent_indent, _)| indentation <= *parent_indent) {
            path.pop();
        }

        let (entry, entry_offset) =
            trimmed.strip_prefix("- ").map_or((trimmed, line.len() - trimmed.len()), |entry| {
                (entry, line.len() - trimmed.len() + 2)
            });
        let Some(separator) = entry.find(':') else {
            offset += segment.len();
            continue;
        };
        let key = normalize_yaml_key(&entry[..separator]);
        let untrimmed_value = &entry[separator + 1..];
        let leading_whitespace = untrimmed_value.len() - untrimmed_value.trim_start().len();
        let raw_value = untrimmed_value.trim_start();
        if raw_value.starts_with(['|', '>']) {
            block_scalar_indent = Some(indentation);
        }
        let is_action_uses = key == "uses" && is_action_uses_path(&path);
        if key != "uses" && (raw_value.is_empty() || raw_value.starts_with('#')) {
            path.push((indentation, key));
        }
        if !is_action_uses || raw_value.is_empty() {
            offset += segment.len();
            continue;
        }
        let value = raw_value.trim_end();
        let start = offset + entry_offset + separator + 1 + leading_whitespace;
        values.push(UsesValue { range: start..start + value.len(), value });
        offset += segment.len();
    }
    values
}

fn normalize_yaml_key(key: &str) -> &str {
    let key = key.trim();
    key.strip_prefix('\'')
        .and_then(|key| key.strip_suffix('\''))
        .or_else(|| key.strip_prefix('"').and_then(|key| key.strip_suffix('"')))
        .unwrap_or(key)
}

fn is_action_uses_path(path: &[(usize, &str)]) -> bool {
    (path.len() == 2 && path[0].1 == "jobs")
        || (path.len() == 3 && path[0].1 == "jobs" && path[2].1 == "steps")
        || (path.len() == 2 && path[0].1 == "runs" && path[1].1 == "steps")
}

fn parse_repo_versions(stdout: &str) -> Vec<RepoVersion> {
    let refs = stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(commit, ref_)| (ref_.to_string(), commit.to_string()))
        .collect::<HashMap<_, _>>();
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
    if let Some(version) = parse_version(&action.ref_)
        && action.ref_.trim_start_matches('v').split('.').count() == 3
    {
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
    } else {
        value.push_str(" # ");
        value.push_str(&target.tag);
    }
    value
}

fn parse_version(input: &str) -> Option<Version> {
    Version::parse(input).or_else(|_| Version::parse(input.trim_start_matches('v'))).ok()
}

fn to_outdated(plans: Vec<PlannedUpdate>, latest: bool) -> Vec<OutdatedGitHubAction> {
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
                homepage: format!("https://github.com/{}", plan.action.repo),
                latest: target.version,
                name: plan.action.name,
                wanted: plan.wanted.version,
            },
        );
    }
    actions.into_values().collect()
}

#[cfg(test)]
mod tests;
