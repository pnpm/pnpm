//! Finding an exact Python patch in historical releases.

use super::{Build, builds_in};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::Config;
use pnpm_crypto_shasums_file::{FetchShasumsFileError, ShasumsFileItem, fetch_shasums_file_cached};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use serde::Deserialize;
use std::cmp::Ordering;

/// The tags endpoint is small because it does not expand each release's
/// hundreds of assets. Two pages currently cover the project's history.
const TAGS_PER_PAGE: usize = 100;
const MAX_TAG_PAGES: usize = 10;
const MAX_TAGS_PAGE_BYTES: usize = 1024 * 1024;

pub(super) async fn historical_build(
    config: &Config,
    client: &ThrottledClient,
    releases_url: &str,
    tags_url: &str,
    exact: &pep440_rs::Version,
) -> Result<Option<Build>> {
    let tags = release_tags(client, tags_url, config.retry_opts()).await?;
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
                let Some(probe) = nearest_host_release(
                    config,
                    client,
                    releases_url,
                    &tags,
                    start..end,
                    middle,
                    exact,
                )
                .await?
                else {
                    return Ok(None);
                };
                if let Some(build) = continue_after_gap(probe, &mut start, &mut end) {
                    return Ok(Some(build));
                }
            }
        }
    }
    Ok(None)
}

enum HostProbe {
    Found(Build),
    Ordered { index: usize, order: Ordering },
}

fn continue_after_gap(probe: HostProbe, start: &mut usize, end: &mut usize) -> Option<Build> {
    match probe {
        HostProbe::Found(build) => Some(build),
        HostProbe::Ordered { index, order: Ordering::Greater } => {
            *start = index + 1;
            None
        }
        HostProbe::Ordered { index, order: Ordering::Less } => {
            *end = index;
            None
        }
        HostProbe::Ordered { order: Ordering::Equal, .. } => {
            unreachable!("an equal host release holds the exact build")
        }
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
) -> Result<Option<HostProbe>> {
    for distance in 1..range.len() {
        let newer = middle
            .checked_sub(distance)
            .filter(|index| *index >= range.start);
        let older = middle
            .checked_add(distance)
            .filter(|index| *index < range.end);
        for index in newer.into_iter().chain(older) {
            let url = format!("{releases_url}/download/{}/SHA256SUMS", tags[index]);
            let Some(release) = release_index(config, client, &url).await? else {
                continue;
            };
            let mut builds = builds_in(&release);
            if let Some(build) = take_exact_build(&mut builds, exact) {
                return Ok(Some(HostProbe::Found(build)));
            }
            let order = release_order(&builds, exact);
            if order != Ordering::Equal {
                return Ok(Some(HostProbe::Ordered { index, order }));
            }
        }
    }
    Ok(None)
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
    match fetch_shasums_file_cached(client, url, Some(&config.cache_dir)).await {
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

async fn release_tags(
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
            tags.sort_unstable_by(|left, right| right.cmp(left));
            tags.dedup();
            return Ok(tags);
        }
    }
    bail!(
        "the Python interpreter release list holds more than {} tags",
        TAGS_PER_PAGE * MAX_TAG_PAGES,
    )
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
