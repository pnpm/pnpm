//! Pacquet port of pnpm's
//! [`resolveNodeVersion` / `resolveNodeVersions`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L185-L221).
//!
//! Pull nodejs.org's `index.json` for a mirror, filter the listed
//! versions, and pick the one (or the set) matching a user-supplied
//! selector. The selector may be:
//!
//! - `latest` — the first entry in the index (the newest published
//!   build on that channel).
//! - `lts` — the newest entry tagged with any LTS codename.
//! - An LTS codename (`argon`, `iron`, ...) — `*` within that codename.
//! - A semver range — pick the `max_satisfying` version.

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_network::ThrottledClient;
use serde::Deserialize;

/// Pattern matched against archive entries pacquet strips out of the
/// Node.js tarball — `npm`, `npx`, and `corepack` ship with Node but
/// pnpm manages its own package managers, so the install layer
/// excludes them per upstream's
/// [`NODE_EXTRAS_IGNORE_PATTERN`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L32).
pub const NODE_EXTRAS_IGNORE_PATTERN: &str = r"^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)";

/// One row of the `index.json` Node.js publishes for each mirror.
///
/// `version` arrives with a leading `v`; pacquet strips it at the
/// deserialization boundary. `lts` is either `false` (non-LTS build)
/// or a string codename — JSON deserialization needs the union shape.
#[derive(Debug, Clone)]
pub(crate) struct NodeVersion {
    pub(crate) version: String,
    pub(crate) lts: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNodeVersion {
    version: String,
    #[serde(default)]
    lts: serde_json::Value,
}

/// Errors raised by [`resolve_node_version`] and [`resolve_node_versions`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveNodeVersionError {
    #[display("Failed to fetch Node.js release index at {url}")]
    #[diagnostic(code(FETCH_NODE_INDEX_FAILED))]
    FetchIndex {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to decode Node.js release index at {url}")]
    #[diagnostic(code(DECODE_NODE_INDEX_FAILED))]
    DecodeIndex {
        url: String,
        #[error(source)]
        error: Arc<serde_json::Error>,
    },
}

/// Pick the single best Node.js version for a selector against a mirror.
///
/// `node_mirror_base_url` falls back to the official `release` channel
/// when `None`; matches upstream's
/// [`fetchAllVersions` default](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L215).
/// Returns `Ok(None)` when the index is reachable but no version
/// satisfies the selector — the caller raises
/// `NODEJS_VERSION_NOT_FOUND` to mirror upstream.
pub async fn resolve_node_version(
    http_client: &ThrottledClient,
    version_spec: &str,
    node_mirror_base_url: Option<&str>,
) -> Result<Option<String>, ResolveNodeVersionError> {
    let all_versions = fetch_all_versions(http_client, node_mirror_base_url).await?;
    if version_spec == "latest" {
        return Ok(all_versions.first().map(|version| version.version.clone()));
    }
    let (versions, range) = filter_versions(&all_versions, version_spec);
    Ok(max_satisfying(&versions, &range))
}

/// Pick every Node.js version a selector accepts against a mirror.
///
/// Used by `pnpm exec` and the `outdated` command to enumerate
/// candidates rather than commit to a single pick. `version_spec`
/// `None` returns the full mirror index in published order.
pub async fn resolve_node_versions(
    http_client: &ThrottledClient,
    version_spec: Option<&str>,
    node_mirror_base_url: Option<&str>,
) -> Result<Vec<String>, ResolveNodeVersionError> {
    let all_versions = fetch_all_versions(http_client, node_mirror_base_url).await?;
    let Some(version_spec) = version_spec else {
        return Ok(all_versions.into_iter().map(|version| version.version).collect());
    };
    if version_spec == "latest" {
        return Ok(all_versions
            .into_iter()
            .next()
            .map(|version| vec![version.version])
            .unwrap_or_default());
    }
    let (versions, range) = filter_versions(&all_versions, version_spec);
    let Ok(parsed_range) = Range::parse(&range) else { return Ok(Vec::new()) };
    Ok(versions
        .into_iter()
        .filter(|version| {
            Version::parse(version)
                .is_ok_and(|parsed| satisfies_with_prereleases(&parsed, &parsed_range))
        })
        .collect())
}

async fn fetch_all_versions(
    http_client: &ThrottledClient,
    node_mirror_base_url: Option<&str>,
) -> Result<Vec<NodeVersion>, ResolveNodeVersionError> {
    let base = node_mirror_base_url.unwrap_or("https://nodejs.org/download/release/");
    let url = format!("{base}index.json");
    let response = http_client
        .acquire_for_url(&url)
        .await
        .get(&url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| ResolveNodeVersionError::FetchIndex {
            url: url.clone(),
            error: Arc::new(error),
        })?;
    let body = response.text().await.map_err(|error| ResolveNodeVersionError::FetchIndex {
        url: url.clone(),
        error: Arc::new(error),
    })?;
    let raw: Vec<RawNodeVersion> = serde_json::from_str(&body).map_err(|error| {
        ResolveNodeVersionError::DecodeIndex { url: url.clone(), error: Arc::new(error) }
    })?;
    Ok(raw
        .into_iter()
        .map(|entry| NodeVersion {
            version: entry.version.strip_prefix('v').unwrap_or(&entry.version).to_string(),
            lts: lts_codename(entry.lts),
        })
        .collect())
}

/// Decode the `lts` field upstream emits as `false | string`.
fn lts_codename(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(name) => Some(name),
        _ => None,
    }
}

/// Reduce the mirror index into the candidate set + matching range
/// for the user's selector. Mirrors upstream's `filterVersions`.
fn filter_versions(versions: &[NodeVersion], version_selector: &str) -> (Vec<String>, String) {
    if version_selector == "lts" {
        return (
            versions
                .iter()
                .filter(|version| version.lts.is_some())
                .map(|version| version.version.clone())
                .collect(),
            "*".to_string(),
        );
    }
    if is_dist_tag(version_selector) {
        let wanted = version_selector.to_ascii_lowercase();
        return (
            versions
                .iter()
                .filter(|version| {
                    version.lts.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(&wanted))
                })
                .map(|version| version.version.clone())
                .collect(),
            "*".to_string(),
        );
    }
    (versions.iter().map(|version| version.version.clone()).collect(), version_selector.to_string())
}

/// Mirrors `versionSelectorType(...)?.type === 'tag'` upstream: the
/// selector is a "tag" only when it parses as neither a `Version` nor
/// a `Range`. We don't run the `encodeURIComponent` punctuation check
/// — `filter_versions`'s only consumer is the LTS-codename branch,
/// and codenames are alphabetic.
fn is_dist_tag(selector: &str) -> bool {
    Version::parse(selector).is_err() && Range::parse(selector).is_err()
}

fn max_satisfying(versions: &[String], range: &str) -> Option<String> {
    let parsed_range = Range::parse(range).ok()?;
    let mut best: Option<(Version, String)> = None;
    for version in versions {
        let Ok(parsed) = Version::parse(version) else { continue };
        if !satisfies_with_prereleases(&parsed, &parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, version.clone())),
        }
    }
    best.map(|(_, raw)| raw)
}

/// Range-satisfaction check that mirrors upstream's
/// `semver.maxSatisfying(versions, range, { includePrerelease: true, … })`
/// call at
/// [`engine/runtime/node-resolver/src/index.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L185-L196).
///
/// `node-semver`'s [`Version::satisfies`] rejects prerelease versions
/// against non-prerelease comparators by default — for example
/// `18.0.0-rc.1` against `^18.0.0` returns `false`. JavaScript's
/// `semver` library exposes an `includePrerelease` opt-in that lets
/// that pairing match, which the upstream node-resolver enables
/// unconditionally. Pacquet approximates the same opt-in by retrying
/// with the prerelease suffix stripped when the straight check fails:
/// if `version` is a prerelease and `MAJOR.MINOR.PATCH` satisfies
/// `range`, treat the candidate as satisfying. Mirrors the strategy
/// already used by `satisfies_with_prereleases` in
/// `resolving-deps-resolver`.
fn satisfies_with_prereleases(version: &Version, range: &Range) -> bool {
    if version.satisfies(range) {
        return true;
    }
    if version.pre_release.is_empty() {
        return false;
    }
    let base = Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    base.satisfies(range)
}

#[cfg(test)]
mod tests;
