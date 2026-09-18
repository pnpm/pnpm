//! Finding an exact Python patch in historical releases.

use super::{Build, builds_in};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::Config;
use pnpm_crypto_shasums_file::{
    FetchShasumsFileError, ShasumsFileItem, fetch_shasums_file_cached_with_retry,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    collections::VecDeque,
    path::{Path, PathBuf},
    time::Duration,
};

/// The tags endpoint is small because it does not expand each release's
/// hundreds of assets. Two pages currently cover the project's history.
const TAGS_PER_PAGE: usize = 100;
const MAX_TAG_PAGES: usize = 10;
const MAX_TAGS_PAGE_BYTES: usize = 1024 * 1024;
const MAX_TAGS_CACHE_BYTES: usize = 64 * 1024;
const TAGS_MAX_AGE: Duration = Duration::from_hours(24);
// Spread a fixed number of probes across a gap so old runs of missing or
// incompatible manifests cannot make one exact pin scan the entire history.
const MAX_HOST_GAP_PROBES: usize = 8;

pub(super) async fn historical_build(
    config: &Config,
    client: &ThrottledClient,
    releases_url: &str,
    tags_url: &str,
    exact: &pep440_rs::Version,
) -> Result<Option<Build>> {
    let tags = release_tags(config, client, tags_url).await?;
    if let Some(build) =
        historical_build_in_tags(config, client, releases_url, &tags.values, exact).await?
    {
        return Ok(Some(build));
    }
    if !tags.from_cache {
        return Ok(None);
    }
    let tags = download_release_tags(client, tags_url, config.retry_opts()).await?;
    write_release_tags_cache(&release_tags_cache_path(config, tags_url), &tags).await;
    historical_build_in_tags(config, client, releases_url, &tags, exact).await
}

async fn historical_build_in_tags(
    config: &Config,
    client: &ThrottledClient,
    releases_url: &str,
    tags: &[String],
    exact: &pep440_rs::Version,
) -> Result<Option<Build>> {
    let mut start = 0;
    let mut end = tags.len();
    while start < end {
        let middle = start + (end - start) / 2;
        let tag = &tags[middle];
        let url = format!("{releases_url}/download/{tag}/SHA256SUMS");
        let index = release_index(config, client, &url).await?;
        let mut builds = index
            .as_deref()
            .map(builds_in)
            .unwrap_or_default();
        if let Some(build) = take_exact_build(&mut builds, exact) {
            return Ok(Some(build));
        }
        match release_order(&builds, exact) {
            Ordering::Greater => start = middle + 1,
            Ordering::Less => end = middle,
            Ordering::Equal => {
                let probe = nearest_host_release(
                    config,
                    client,
                    releases_url,
                    tags,
                    start..end,
                    middle,
                    exact,
                )
                .await?;
                if let GapSearch::Done(build) =
                    continue_after_gap(probe, &mut start, &mut end, exact)?
                {
                    return Ok(build);
                }
            }
        }
    }
    Ok(None)
}

enum HostProbe {
    Found(Build),
    Ordered { index: usize, order: Ordering },
    Absent,
    BudgetExhausted,
}

enum GapSearch {
    Continue,
    Done(Option<Build>),
}

fn continue_after_gap(
    probe: HostProbe,
    start: &mut usize,
    end: &mut usize,
    exact: &pep440_rs::Version,
) -> Result<GapSearch> {
    match probe {
        HostProbe::Found(build) => Ok(GapSearch::Done(Some(build))),
        HostProbe::Ordered { index, order: Ordering::Greater } => {
            *start = index + 1;
            Ok(GapSearch::Continue)
        }
        HostProbe::Ordered { index, order: Ordering::Less } => {
            *end = index;
            Ok(GapSearch::Continue)
        }
        HostProbe::Ordered { order: Ordering::Equal, .. } => {
            unreachable!("an equal host release holds the exact build")
        }
        HostProbe::Absent => Ok(GapSearch::Done(None)),
        HostProbe::BudgetExhausted => Err(miette::miette!(
            code = "ERR_PNPM_PYTHON_RELEASE_LOOKUP_LIMIT",
            help = "Use a less specific Python version requirement or retry after the upstream release layout changes.",
            "cannot determine whether Python {exact} is available after sampling {MAX_HOST_GAP_PROBES} other releases that omit this platform",
        )),
    }
}

async fn nearest_host_release(
    config: &Config,
    client: &ThrottledClient,
    releases_url: &str,
    tags: &[String],
    range: std::ops::Range<usize>,
    middle: usize,
    exact: &pep440_rs::Version,
) -> Result<HostProbe> {
    let budget_exhausted = range.len().saturating_sub(1) > MAX_HOST_GAP_PROBES;
    for index in gap_probe_candidates(range.clone(), middle) {
        let url = format!("{releases_url}/download/{}/SHA256SUMS", tags[index]);
        let Some(release) = release_index(config, client, &url).await? else {
            continue;
        };
        let mut builds = builds_in(&release);
        if let Some(build) = take_exact_build(&mut builds, exact) {
            return Ok(HostProbe::Found(build));
        }
        let order = release_order(&builds, exact);
        if order != Ordering::Equal {
            return Ok(HostProbe::Ordered { index, order });
        }
    }
    Ok(if budget_exhausted { HostProbe::BudgetExhausted } else { HostProbe::Absent })
}

fn gap_probe_candidates(range: std::ops::Range<usize>, middle: usize) -> Vec<usize> {
    let mut intervals = VecDeque::from([range.start..middle, middle + 1..range.end]);
    let mut candidates = Vec::with_capacity(MAX_HOST_GAP_PROBES);
    while let Some(interval) = intervals.pop_front()
        && candidates.len() < MAX_HOST_GAP_PROBES
    {
        if interval.is_empty() {
            continue;
        }
        let candidate = interval.start + interval.len() / 2;
        candidates.push(candidate);
        intervals.push_back(interval.start..candidate);
        intervals.push_back(candidate + 1..interval.end);
    }
    candidates
}

fn take_exact_build(builds: &mut Vec<Build>, exact: &pep440_rs::Version) -> Option<Build> {
    let position = builds
        .iter()
        .position(|build| build.version == *exact)?;
    Some(builds.swap_remove(position))
}

async fn release_index(
    config: &Config,
    client: &ThrottledClient,
    url: &str,
) -> Result<Option<Vec<ShasumsFileItem>>> {
    match fetch_shasums_file_cached_with_retry(
        client,
        url,
        Some(&config.cache_dir),
        config.retry_opts(),
    )
    .await
    {
        Ok(index) => Ok(Some(index)),
        Err(FetchShasumsFileError::StatusNotOk { status: 404, .. }) => Ok(None),
        Err(error) => {
            Err(error).wrap_err_with(|| format!("read the Python interpreters {url} offers"))
        }
    }
}

/// Whether this machine's builds in a release are newer or older than
/// `exact`. Python versions move monotonically across the date-ordered
/// tags, which makes finding one immutable checksum file logarithmic rather
/// than a request per release.
fn release_order(builds: &[Build], exact: &pep440_rs::Version) -> Ordering {
    builds
        .iter()
        .map(|build| &build.version)
        .filter(|version| version.release().get(..2) == exact.release().get(..2))
        .max()
        .or_else(|| {
            builds
                .iter()
                .map(|build| &build.version)
                .max()
        })
        .map_or(Ordering::Equal, |offered| offered.cmp(exact))
}

#[derive(Deserialize)]
struct GithubTag {
    name: String,
}

struct ReleaseTags {
    values: Vec<String>,
    from_cache: bool,
}

async fn release_tags(
    config: &Config,
    client: &ThrottledClient,
    tags_url: &str,
) -> Result<ReleaseTags> {
    let cache = release_tags_cache_path(config, tags_url);
    if let Some(tags) = read_cached_release_tags(&cache, TAGS_MAX_AGE).await {
        return Ok(ReleaseTags { values: tags, from_cache: true });
    }
    let tags = download_release_tags(client, tags_url, config.retry_opts()).await?;
    write_release_tags_cache(&cache, &tags).await;
    Ok(ReleaseTags { values: tags, from_cache: false })
}

async fn download_release_tags(
    client: &ThrottledClient,
    tags_url: &str,
    retry_opts: RetryOpts,
) -> Result<Vec<String>> {
    let mut tags = Vec::new();
    for page in 1..=MAX_TAG_PAGES {
        let url = format!("{tags_url}?per_page={TAGS_PER_PAGE}&page={page}");
        let page = release_tags_page(client, &url, retry_opts).await?;
        let is_last = page.len() < TAGS_PER_PAGE;
        tags.extend(
            page.into_iter()
                .map(|tag| tag.name)
                .filter(|tag| valid_tag(tag)),
        );
        if is_last {
            return Ok(canonical_release_tags(tags));
        }
    }
    bail!(
        "the Python interpreter release list holds more than {} tags",
        TAGS_PER_PAGE * MAX_TAG_PAGES,
    )
}

fn release_tags_cache_path(config: &Config, tags_url: &str) -> PathBuf {
    config.cache_dir
        .join("python-release-tags-v1")
        .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(tags_url)))
}

pub(super) async fn read_cached_release_tags(
    path: &Path,
    max_age: Duration,
) -> Option<Vec<String>> {
    let age = tokio::fs::metadata(path).await
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    if age >= max_age {
        return None;
    }
    let tags: Vec<String> =
        crate::cache::read_json(path, MAX_TAGS_CACHE_BYTES, "Python interpreter release tag cache")
            .await
            .ok()?;
    (tags.len() <= TAGS_PER_PAGE * MAX_TAG_PAGES)
        .then(|| canonical_release_tags(tags))
        .filter(|tags| !tags.is_empty())
}

async fn write_release_tags_cache(path: &Path, tags: &[String]) {
    let Some(parent) = path.parent() else { return };
    let Ok(body) = serde_json::to_vec(tags) else {
        return;
    };
    if body.len() > MAX_TAGS_CACHE_BYTES
        || tokio::fs::create_dir_all(parent).await.is_err()
    {
        return;
    }
    let path = path.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || pnpm_fs::write_atomic(&path, &body)).await;
}

fn canonical_release_tags(mut tags: Vec<String>) -> Vec<String> {
    tags.retain(|tag| valid_tag(tag));
    tags.sort_unstable_by(|left, right| right.cmp(left));
    tags.dedup();
    tags
}

async fn release_tags_page(
    client: &ThrottledClient,
    url: &str,
    retry_opts: RetryOpts,
) -> Result<Vec<GithubTag>> {
    let response = client
        .get_limited_bytes_with_secure_auth_and_retry(
            url,
            &AuthHeaders::default(),
            None,
            retry_opts,
            MAX_TAGS_PAGE_BYTES,
        )
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("read the Python interpreter release tags from {url}"))?;
    if response.body_truncated {
        bail!("the Python interpreter release tags at {url} exceed {MAX_TAGS_PAGE_BYTES} bytes");
    }
    if !response.status.is_success() {
        bail!(
            "reading the Python interpreter release tags from {url} returned {}",
            response.status,
        );
    }
    serde_json::from_slice(&response.body)
        .into_diagnostic()
        .wrap_err_with(|| format!("read the Python interpreter release tags from {url}"))
}

fn valid_tag(tag: &str) -> bool {
    tag.len() == 8 && tag.bytes().all(|byte| byte.is_ascii_digit())
}
