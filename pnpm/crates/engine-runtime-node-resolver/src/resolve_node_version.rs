//! Resolves a user-supplied version selector to a concrete Node.js
//! version (or set of versions).
//!
//! Pull nodejs.org's `index.json` for a mirror, filter the listed
//! versions, and pick the one (or the set) matching a user-supplied
//! selector. The selector may be:
//!
//! - `latest` or an empty selector, the newest stable entry in the
//!   index, or the first entry when the index has no stable entry (the
//!   newest build on a prerelease channel such as `nightly`).
//! - `lts`, the newest entry tagged with any LTS codename.
//! - An LTS codename (`argon`, `iron`, ...), `*` within that codename.
//! - A semver range, pick the [`max_satisfying_stable_first`] version.

use std::sync::Arc;

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use node_semver::{
    Range,
    Version,
};
use pnpm_network::{
    AuthHeaders,
    ThrottledClient,
};
use serde::Deserialize;

/// Pattern matched against archive entries pacquet strips out of the
/// Node.js tarball, `npm`, `npx`, and `corepack` ship with Node but
/// pnpm manages its own package managers, so the install layer
/// excludes them.
pub const NODE_EXTRAS_IGNORE_PATTERN: &str = r"^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)";

/// One row of the `index.json` Node.js publishes for each mirror.
#[derive(Debug, Clone)]
pub struct NodeRelease {
    /// The version without the leading `v` the index spells it with.
    pub version: String,
    /// The LTS codename. The index writes `false` for a non-LTS build.
    pub lts: Option<String>,
    /// The row as the index publishes it.
    pub index_entry: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawNodeRelease {
    version: String,
    #[serde(default)]
    lts: serde_json::Value,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

/// Errors raised by [`resolve_node_version`] and [`resolve_node_versions`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveNodeVersionError {
    #[display("Failed to fetch Node.js release index at {url}")]
    #[diagnostic(code(ERR_PNPM_FETCH_NODE_INDEX_FAILED))]
    FetchIndex {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to fetch Node.js release index at {url}: HTTP status {status}")]
    #[diagnostic(code(ERR_PNPM_FETCH_NODE_INDEX_FAILED))]
    FetchIndexStatus { url: String, status: u16 },

    #[display("Failed to decode Node.js release index at {url}")]
    #[diagnostic(code(ERR_PNPM_DECODE_NODE_INDEX_FAILED))]
    DecodeIndex {
        url: String,
        #[error(source)]
        error: Arc<serde_json::Error>,
    },
}

/// Pick the single best Node.js version for a selector against a mirror.
///
/// `node_mirror_base_url` falls back to the official `release` channel
/// when `None`. Returns `Ok(None)` when the index is reachable but no
/// version satisfies the selector, the caller raises
/// `ERR_PNPM_NODEJS_VERSION_NOT_FOUND`.
pub async fn resolve_node_version(
    http_client: &ThrottledClient,
    version_spec: &str,
    node_mirror_base_url: Option<&str>,
) -> Result<Option<String>, ResolveNodeVersionError> {
    resolve_node_version_with_auth(
        http_client,
        &AuthHeaders::default(),
        version_spec,
        node_mirror_base_url,
    )
    .await
}

/// Like [`resolve_node_version`], authenticating the release-index request
/// with the longest URL-prefix match from [`AuthHeaders`].
pub async fn resolve_node_version_with_auth(
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    version_spec: &str,
    node_mirror_base_url: Option<&str>,
) -> Result<Option<String>, ResolveNodeVersionError> {
    let releases = fetch_node_releases(http_client, auth_headers, node_mirror_base_url).await?;
    Ok(pick_node_version(&releases, version_spec))
}

/// Pick the single best version for a selector from a mirror's
/// releases, as [`resolve_node_version`] does.
#[must_use]
pub fn pick_node_version(releases: &[NodeRelease], version_spec: &str) -> Option<String> {
    if is_latest_selector(version_spec) {
        return latest_version(releases);
    }
    let (versions, range) = filter_versions(releases, version_spec);
    max_satisfying_stable_first(&versions, &range)
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
    resolve_node_versions_with_auth(
        http_client,
        &AuthHeaders::default(),
        version_spec,
        node_mirror_base_url,
    )
    .await
}

/// Like [`resolve_node_versions`], authenticating the release-index request
/// with the longest URL-prefix match from [`AuthHeaders`].
pub async fn resolve_node_versions_with_auth(
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    version_spec: Option<&str>,
    node_mirror_base_url: Option<&str>,
) -> Result<Vec<String>, ResolveNodeVersionError> {
    let all_versions = fetch_node_releases(http_client, auth_headers, node_mirror_base_url).await?;
    let Some(version_spec) = version_spec else {
        return Ok(all_versions
            .into_iter()
            .map(|version| version.version)
            .collect());
    };
    if is_latest_selector(version_spec) {
        return Ok(latest_version(&all_versions).into_iter().collect());
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

fn latest_version(versions: &[NodeRelease]) -> Option<String> {
    versions
        .iter()
        .find(|version| {
            Version::parse(&version.version)
                .is_ok_and(|parsed| parsed.pre_release.is_empty())
        })
        .or_else(|| versions.first())
        .map(|version| version.version.clone())
}

/// Fetch a mirror's release index, newest release first.
///
/// `node_mirror_base_url` falls back to the official `release` channel
/// when `None`.
pub async fn fetch_node_releases(
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    node_mirror_base_url: Option<&str>,
) -> Result<Vec<NodeRelease>, ResolveNodeVersionError> {
    let base = node_mirror_base_url.unwrap_or("https://nodejs.org/download/release/");
    let url = format!("{base}index.json");
    let response = http_client
        .get_bytes_with_secure_auth_headers(&url, auth_headers)
        .await
        .map_err(|error| ResolveNodeVersionError::FetchIndex {
            url: url.clone(),
            error: Arc::new(error),
        })?;
    if !response.status.is_success() {
        return Err(ResolveNodeVersionError::FetchIndexStatus {
            url,
            status: response.status.as_u16(),
        });
    }
    let raw: Vec<RawNodeRelease> = serde_json::from_slice(&response.body)
        .map_err(|error| ResolveNodeVersionError::DecodeIndex {
            url: url.clone(),
            error: Arc::new(error),
        })?;
    Ok(raw
        .into_iter()
        .map(|entry| {
            let version = entry.version
                .strip_prefix('v')
                .unwrap_or(&entry.version)
                .to_string();
            let mut index_entry = entry.rest;
            index_entry.insert("version".to_string(), entry.version.into());
            index_entry.insert("lts".to_string(), entry.lts.clone());
            NodeRelease { version, lts: lts_codename(entry.lts), index_entry }
        })
        .collect())
}

fn is_latest_selector(version_spec: &str) -> bool {
    let version_spec = version_spec.trim();
    version_spec.is_empty() || version_spec == "latest"
}

/// Decode the index's `lts` field, which is `false | string`.
fn lts_codename(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(name) => Some(name),
        _ => None,
    }
}

/// Reduce the mirror index into the candidate set + matching range
/// for the user's selector.
fn filter_versions(versions: &[NodeRelease], version_selector: &str) -> (Vec<String>, String) {
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
                    version.lts
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&wanted))
                })
                .map(|version| version.version.clone())
                .collect(),
            "*".to_string(),
        );
    }
    (
        versions
            .iter()
            .map(|version| version.version.clone())
            .collect(),
        version_selector.to_string(),
    )
}

/// A selector is a "tag" only when it parses as neither a `Version`
/// nor a `Range`. The punctuation check that a full dist-tag validator
/// would run is skipped, [`filter_versions`]'s only consumer is the
/// LTS-codename branch, and codenames are alphabetic.
fn is_dist_tag(selector: &str) -> bool {
    Version::parse(selector).is_err() && Range::parse(selector).is_err()
}

fn max_satisfying_stable_first(versions: &[String], range: &str) -> Option<String> {
    let stable_versions: Vec<String> = versions
        .iter()
        .filter(|version| {
            Version::parse(version).is_ok_and(|parsed| parsed.pre_release.is_empty())
        })
        .cloned()
        .collect();
    max_satisfying(&stable_versions, range).or_else(|| max_satisfying(versions, range))
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

/// Range-satisfaction check with `includePrerelease`-style semantics:
/// pick the max satisfying version, letting prereleases match
/// non-prerelease comparators.
///
/// `node-semver`'s [`Version::satisfies`] rejects prerelease versions
/// against non-prerelease comparators by default, for example
/// `18.0.0-rc.1` against `^18.0.0` returns `false`. JavaScript's
/// `semver` library exposes an `includePrerelease` opt-in that lets
/// that pairing match, which node version resolution enables
/// unconditionally. Pacquet approximates the same opt-in by retrying
/// with the prerelease suffix stripped when the straight check fails:
/// if `version` is a prerelease and `MAJOR.MINOR.PATCH` satisfies
/// `range`, treat the candidate as satisfying. Same strategy used by
/// [`satisfies_with_prereleases`] in `resolving-deps-resolver`.
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
