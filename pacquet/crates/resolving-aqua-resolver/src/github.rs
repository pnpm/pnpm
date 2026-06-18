//! Pacquet port of upstream's
//! [`resolving/aqua-resolver/src/github.ts`](https://github.com/pnpm/pnpm/pull/10970)
//! (co-developed in the same PR).
//!
//! Resolves a release tag through the GitHub Releases API and fetches
//! the per-release checksum files referenced by the aqua-registry.

use std::{collections::BTreeMap, sync::Arc};

use pacquet_network::ThrottledClient;
use serde::Deserialize;

use crate::error::AquaResolverError;

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

/// Resolve `version_spec` to a concrete release tag, or the latest
/// release when no version was requested.
///
/// A requested version is tried as both `vX.Y.Z` and `X.Y.Z` to match
/// the two tag conventions GitHub projects use.
pub async fn resolve_github_version(
    http_client: &ThrottledClient,
    owner: &str,
    repo: &str,
    version_spec: Option<&str>,
) -> Result<String, AquaResolverError> {
    if let Some(version_spec) = version_spec {
        let tag = if version_spec.starts_with('v') {
            version_spec.to_string()
        } else {
            format!("v{version_spec}")
        };
        if check_release_exists(http_client, owner, repo, &tag).await? {
            return Ok(tag);
        }
        if !version_spec.starts_with('v')
            && check_release_exists(http_client, owner, repo, version_spec).await?
        {
            return Ok(version_spec.to_string());
        }
        return Err(AquaResolverError::VersionNotFound {
            owner: owner.to_string(),
            repo: repo.to_string(),
            version_spec: version_spec.to_string(),
        });
    }

    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/latest");
    let response = github_get(http_client, &url).await?;
    if !response.status().is_success() {
        return Err(AquaResolverError::GitHubFetch {
            what: format!("latest release for {owner}/{repo}"),
            status: response.status().as_u16(),
        });
    }
    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|error| AquaResolverError::Network { url: url.clone(), error: Arc::new(error) })?;
    Ok(release.tag_name)
}

async fn check_release_exists(
    http_client: &ThrottledClient,
    owner: &str,
    repo: &str,
    tag: &str,
) -> Result<bool, AquaResolverError> {
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/tags/{tag}");
    let response = github_get(http_client, &url).await?;
    Ok(response.status().is_success())
}

/// Download a release's checksum file and decode its `hash filename`
/// rows. A missing file (non-success status) yields an empty map so the
/// caller falls back to no integrity, mirroring upstream.
pub async fn fetch_checksum_file(
    http_client: &ThrottledClient,
    owner: &str,
    repo: &str,
    tag: &str,
    checksum_asset_name: &str,
) -> Result<BTreeMap<String, String>, AquaResolverError> {
    let url =
        format!("https://github.com/{owner}/{repo}/releases/download/{tag}/{checksum_asset_name}");
    let response =
        http_client.acquire_for_url(&url).await.get(&url).send().await.map_err(|error| {
            AquaResolverError::Network { url: url.clone(), error: Arc::new(error) }
        })?;
    if !response.status().is_success() {
        return Ok(BTreeMap::new());
    }
    let text = response
        .text()
        .await
        .map_err(|error| AquaResolverError::Network { url: url.clone(), error: Arc::new(error) })?;
    Ok(parse_checksum_file(&text))
}

/// Parse a checksum file into a `filename -> hash` map. A single-column
/// file (one hash, no filename) is stored under the empty-string key, so
/// callers can look up a per-asset `.sha256` sidecar by either name.
pub fn parse_checksum_file(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        match parts.as_slice() {
            [hash] => {
                map.insert(String::new(), (*hash).to_string());
            }
            [hash, .., file_name] => {
                map.insert((*file_name).to_string(), (*hash).to_string());
            }
            [] => {}
        }
    }
    map
}

async fn github_get(
    http_client: &ThrottledClient,
    url: &str,
) -> Result<reqwest::Response, AquaResolverError> {
    // Bind the guard so its concurrency permit is held until `send`
    // completes; the request builder clones the inner client, so it
    // outlives the guard's borrow.
    let guard = http_client.acquire_for_url(url).await;
    let mut request = guard.get(url).header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.send().await.map_err(|error| AquaResolverError::Network {
        url: url.to_string(),
        error: Arc::new(error),
    })
}

#[cfg(test)]
mod tests;
