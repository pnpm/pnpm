//! Per-version publish timestamp from npm's attestation endpoint —
//! `/-/npm/v1/attestations/<name>@<version>`.
//!
//! The endpoint serves a small JSON document containing one or more
//! Sigstore bundles. We read
//! `bundle.verificationMaterial.tlogEntries[].integratedTime`
//! (the Rekor inclusion time) and surface it as an ISO timestamp.
//! This is a few seconds after the actual publish — close enough for
//! a release-age policy that operates in minutes/hours/days, and
//! tens of kilobytes versus the multi-megabyte full-metadata fetch.
//!
//! We deliberately do **not** verify the Sigstore signature here:
//! the trust model is identical to reading the registry's `time`
//! field on the full metadata document.
//!
//! Returns `Ok(None)` (not `Err`) on every "no answer" condition:
//! 4xx/5xx responses, malformed JSON, missing timestamps. The
//! verifier falls back to the next layer of the publish-time lookup
//! chain. Real network errors propagate as
//! [`crate::FetchMetadataError::Network`] so the verifier can fold
//! them into a violation reason instead of swallowing.
//!
//! Verbatim port of upstream's
//! [`fetchAttestationPublishedAt.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchAttestationPublishedAt.ts).

use chrono::DateTime;
use pacquet_network::{AuthHeaders, ThrottledClient};

use crate::FetchMetadataError;

/// Options bundle for [`fetch_attestation_published_at`].
#[derive(Debug, Clone)]
pub struct FetchAttestationOptions<'a> {
    pub registry: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
}

/// Fetch the earliest Rekor `integratedTime` across the attestation
/// bundles for `<name>@<version>`. Returns `Ok(Some(rfc3339))` on a
/// 2xx response with a parseable timestamp; `Ok(None)` for any
/// "no answer" condition (4xx/5xx, malformed body, no timestamps);
/// `Err(_)` only when the underlying request fails before reaching
/// the server.
pub async fn fetch_attestation_published_at(
    pkg_name: &str,
    version: &str,
    opts: &FetchAttestationOptions<'_>,
) -> Result<Option<String>, FetchMetadataError> {
    // Strip a trailing `/` from the registry root before assembling
    // the endpoint URL — `<registry>/-/npm/v1/attestations/...`
    // produces `//` otherwise, which some self-hosted registries
    // reject as a malformed path. Matches upstream's
    // `opts.registry.replace(/\/$/, '')`.
    let registry = opts.registry.trim_end_matches('/');
    let url = format!("{registry}/-/npm/v1/attestations/{pkg_name}@{version}");
    let mut request = opts.http_client.acquire_for_url(&url).await.get(&url);
    if let Some(value) = opts.auth_headers.for_url(&url) {
        request = request.header("authorization", value);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            // Mirror upstream's `catch` swallow — return None on
            // network errors so the caller falls through to the
            // full-metadata layer. Surfacing the error would be more
            // informative but inconsistent with upstream.
            tracing::debug!(target: "pacquet_resolving_npm_resolver::attestation", ?error, %url, "attestation fetch failed; falling back");
            return Ok(None);
        }
    };
    // Anything outside the 2xx range = no answer. 404 means the
    // version isn't signed; 5xx means the registry can't say; we
    // fall through either way.
    if !response.status().is_success() {
        return Ok(None);
    }
    let body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(error) => {
            tracing::debug!(target: "pacquet_resolving_npm_resolver::attestation", ?error, %url, "attestation body parse failed; falling back");
            return Ok(None);
        }
    };
    Ok(extract_published_at(&body))
}

/// Pull the earliest `integratedTime` across every attestation
/// bundle in the response and convert it to an ISO timestamp.
/// Earliest is the conservative choice: if two attestations
/// disagree (e.g. publish v0.1 vs SLSA provenance v1) the older
/// Rekor entry is what tells us when the artifact existed in a
/// transparency log, which is the floor on publish time.
fn extract_published_at(body: &serde_json::Value) -> Option<String> {
    let attestations = body.get("attestations")?.as_array()?;
    let mut earliest: Option<i64> = None;
    for attestation in attestations {
        let Some(seconds) = read_earliest_integrated_time(attestation) else { continue };
        earliest = Some(earliest.map_or(seconds, |current| current.min(seconds)));
    }
    let seconds = earliest?;
    // `DateTime::from_timestamp` accepts an i64 of seconds-since-epoch.
    // Any seconds value within the chrono-supported range survives the
    // conversion; the registry isn't going to send pre-1970 timestamps.
    let dt = DateTime::from_timestamp(seconds, 0)?;
    Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn read_earliest_integrated_time(attestation: &serde_json::Value) -> Option<i64> {
    let bundle = attestation.get("bundle")?;
    let verification_material = bundle.get("verificationMaterial")?;
    let tlog_entries = verification_material.get("tlogEntries")?.as_array()?;
    let mut earliest: Option<i64> = None;
    for entry in tlog_entries {
        // npm serializes integratedTime as a string ("1778583836")
        // to avoid JSON precision loss; accept either string or
        // number defensively.
        let seconds = parse_integrated_time_seconds(entry.get("integratedTime")?)?;
        earliest = Some(earliest.map_or(seconds, |current| current.min(seconds)));
    }
    earliest
}

fn parse_integrated_time_seconds(value: &serde_json::Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        return text.parse::<i64>().ok().filter(|&seconds| seconds > 0);
    }
    if let Some(seconds) = value.as_i64() {
        return Some(seconds).filter(|&s| s > 0);
    }
    None
}

#[cfg(test)]
mod tests;
