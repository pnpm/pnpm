//! `minimumReleaseAge` for GitHub Actions. `git ls-remote` lists tags without
//! dates, so the tags the policy has to judge are fetched shallowly and
//! without trees into a scratch repository, where each one's creation date
//! can be read: the tagger date of an annotated tag, the committer date of a
//! lightweight one.
//!
//! Whoever creates a tag or commit sets these dates, and the server does not
//! check them, so a backdated tag passes. Unlike a registry's publish time,
//! they hold back only releases that carry their real date.

use crate::{ActionReference, GIT_CONCURRENCY, RepoVersion, find_current, global_warn};
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use node_semver::Version;
use pnpm_config::version_policy::{PackageVersionPolicy, PolicyMatch};
use pnpm_network::redact_and_sanitize;
use pnpm_reporter::Reporter;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    future::Future,
    io::Write,
    path::Path,
    pin::Pin,
    process::{Command, Stdio},
};

/// The `minimumReleaseAge` policy applied to GitHub Actions versions.
pub struct ReleaseAge {
    /// Versions tagged after this instant are not offered.
    pub published_by: DateTime<Utc>,
    /// `minimumReleaseAgeExclude`, matched against the action name and its
    /// repository.
    pub exclude: Option<PackageVersionPolicy>,
}

impl ReleaseAge {
    fn exempts(&self, action: &ActionReference, version: &Version) -> bool {
        let Some(exclude) = &self.exclude else { return false };
        [&action.name, &action.repo]
            .into_iter()
            .any(|name| match exclude.matches(name) {
                PolicyMatch::No => false,
                PolicyMatch::AnyVersion => true,
                PolicyMatch::ExactVersions(versions) => lists_version(&versions, version),
            })
    }

    fn admits(&self, created_at: Option<i64>) -> bool {
        created_at.is_some_and(|created_at| created_at <= self.published_by.timestamp())
    }

    /// Whether `candidate` is a version newer than `current` whose age the
    /// policy has to judge.
    fn judges(
        &self,
        action: &ActionReference,
        current: &RepoVersion,
        candidate: &RepoVersion,
    ) -> bool {
        candidate.version > current.version
            && (!current.version.pre_release.is_empty()
                || candidate.version.pre_release.is_empty())
            && !self.exempts(action, &candidate.version)
    }
}

fn lists_version(versions: &[String], version: &Version) -> bool {
    versions
        .iter()
        .filter_map(|listed| Version::parse(listed).ok())
        .any(|listed| &listed == version)
}

/// The `minimumReleaseAge` policy together with the source of the tag dates
/// it judges.
#[derive(Clone, Copy)]
pub(crate) struct ReleaseAgeCheck<'a> {
    pub(crate) policy: &'a ReleaseAge,
    pub(crate) dates: &'a dyn TagDateReader,
}

impl<'a> ReleaseAgeCheck<'a> {
    /// The creation dates of the tags newer than the version an action is
    /// on, in every repository with such an action. A repository whose dates
    /// cannot be read is dropped from `refs_by_repo` with a warning, so none
    /// of its versions is offered without its age being known.
    async fn read_dates<Reporter: self::Reporter>(
        self,
        actions: &[ActionReference],
        refs_by_repo: &mut HashMap<String, Vec<RepoVersion>>,
        server_url: &str,
    ) -> ReleaseDates<'a> {
        let tags_by_repo = self.tags_to_date(actions, refs_by_repo);
        let results = stream::iter(tags_by_repo)
            .map(|(repo, tags)| async move {
                let url = format!("{server_url}/{repo}.git");
                let dates = self.dates.read_tag_dates(&url, &tags).await;
                (repo, dates)
            })
            .buffer_unordered(GIT_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        let mut by_repo = HashMap::new();
        for (repo, dates) in results {
            match dates {
                Ok(dates) => {
                    by_repo.insert(repo, dates);
                }
                Err(error) => {
                    global_warn::<Reporter>(redact_and_sanitize(&format!(
                        r#"Skipping the GitHub Actions from "{repo}": cannot read the release dates that minimumReleaseAge needs: {error}"#,
                    )));
                    refs_by_repo.remove(&repo);
                }
            }
        }
        ReleaseDates { policy: Some(self.policy), by_repo }
    }

    fn tags_to_date(
        &self,
        actions: &[ActionReference],
        refs_by_repo: &HashMap<String, Vec<RepoVersion>>,
    ) -> BTreeMap<String, Vec<String>> {
        let mut tags_by_repo = BTreeMap::<String, BTreeSet<String>>::new();
        for action in actions {
            let Some(versions) = refs_by_repo.get(&action.repo) else { continue };
            let Some(current) = find_current(action, versions) else { continue };
            let tags = versions
                .iter()
                .filter(|candidate| self.policy.judges(action, &current, candidate))
                .map(|candidate| candidate.tag.clone());
            tags_by_repo
                .entry(action.repo.clone())
                .or_default()
                .extend(tags);
        }
        tags_by_repo
            .into_iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(repo, tags)| (repo, tags.into_iter().collect()))
            .collect()
    }
}

/// The tag dates a [`ReleaseAgeCheck`] read, and which versions they let
/// through.
pub(crate) struct ReleaseDates<'a> {
    policy: Option<&'a ReleaseAge>,
    by_repo: HashMap<String, TagDates>,
}

impl<'a> ReleaseDates<'a> {
    /// The dates `check` reads, or no restriction without a policy. See
    /// [`ReleaseAgeCheck::read_dates`].
    pub(crate) async fn read<Reporter: self::Reporter>(
        check: Option<ReleaseAgeCheck<'a>>,
        actions: &[ActionReference],
        refs_by_repo: &mut HashMap<String, Vec<RepoVersion>>,
        server_url: &str,
    ) -> Self {
        match check {
            Some(check) => check.read_dates::<Reporter>(actions, refs_by_repo, server_url).await,
            None => Self { policy: None, by_repo: HashMap::new() },
        }
    }

    /// Whether `candidate`, a version newer than the one `action` is on, may
    /// be offered.
    pub(crate) fn admits(&self, action: &ActionReference, candidate: &RepoVersion) -> bool {
        let Some(policy) = self.policy else { return true };
        policy.exempts(action, &candidate.version)
            || policy.admits(
                self.by_repo
                    .get(&action.repo)
                    .and_then(|dates| dates.get(&candidate.tag))
                    .copied(),
            )
    }
}

pub(crate) type TagDates = HashMap<String, i64>;

/// Reads the creation dates, as Unix timestamps, of tags of a repository.
pub(crate) trait TagDateReader: Sync {
    fn read_tag_dates<'a>(
        &'a self,
        repo_url: &'a str,
        tags: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<TagDates, String>> + Send + 'a>>;
}

pub(crate) struct GitTagDates;

impl TagDateReader for GitTagDates {
    fn read_tag_dates<'a>(
        &'a self,
        repo_url: &'a str,
        tags: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<TagDates, String>> + Send + 'a>> {
        let repo_url = repo_url.to_string();
        let tags = tags.to_vec();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || read_tag_dates_blocking(&repo_url, &tags))
                .await
                .map_err(|error| format!("reading tag dates panicked: {error}"))?
        })
    }
}

fn read_tag_dates_blocking(repo_url: &str, tags: &[String]) -> Result<TagDates, String> {
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    run_git(scratch.path(), &["init", "--quiet", "--bare"], None)?;
    let refspecs = tags
        .iter()
        .map(|tag| format!("refs/tags/{tag}:refs/tags/{tag}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fetch = [
        "fetch",
        "--quiet",
        "--depth=1",
        "--filter=tree:0",
        "--no-tags",
        "--no-write-fetch-head",
        "--stdin",
        repo_url,
    ];
    // One retry, like `git ls-remote`.
    if run_git(scratch.path(), &fetch, Some(&refspecs)).is_err() {
        run_git(scratch.path(), &fetch, Some(&refspecs))?;
    }
    let listing = run_git(
        scratch.path(),
        &["for-each-ref", "--format=%(creatordate:unix) %(refname:strip=2)", "refs/tags"],
        None,
    )?;
    Ok(parse_tag_dates(&listing))
}

fn parse_tag_dates(listing: &str) -> TagDates {
    listing
        .lines()
        .filter_map(|line| {
            let (created_at, tag) = line.split_once(' ')?;
            Some((tag.to_string(), created_at.parse().ok()?))
        })
        .collect()
}

fn run_git(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    let mut command = Command::new("git");
    // An inherited repository location would point these commands at the
    // user's repository instead of the scratch one.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    pnpm_git_utils::disable_git_prompts::<pnpm_git_utils::Host>(&mut command, None);
    command
        .current_dir(dir)
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        pipe.write_all(input.as_bytes()).map_err(|error| error.to_string())?;
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests;
