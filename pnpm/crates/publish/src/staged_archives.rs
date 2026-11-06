//! Uploading a batch's archives to pnpr before the commit that references
//! them, so the commit carries each archive's integrity instead of its
//! bytes.
//!
//! pnpr stages an archive uploaded to
//! `PUT <registry>/-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` for the
//! account that uploaded it, and answers `HEAD` on the same URL when it is
//! staged already, so a resumed publish uploads only what is missing.

use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network::{ThrottledClient, read_limited_body};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::registry_config_keys::NormalizedRegistryUrl;

const STAGING_ENDPOINT: &str = "-/pnpr/v0/artifacts/blob";

/// The most staged archives one pnpr commit may reference. A batch with more
/// sends its archives inline.
pub(crate) const MAX_STAGED_ARCHIVES_PER_PUBLISH: usize = 256;

/// Uploads in flight at once. pnpr receives two uploads of an account at a
/// time and holds the rest unanswered, so sending more only risks a queued
/// upload outwaiting the read timeout.
const CONCURRENT_UPLOADS: usize = 2;

/// The most of a refused upload's response body kept for its error.
const ERROR_BODY_LIMIT: usize = 4096;

/// One archive to stage: its bytes and their SRI SHA-512.
pub(crate) struct StagedArchive<'a> {
    pub(crate) integrity: String,
    pub(crate) bytes: &'a [u8],
}

/// Make every attachment of `document` reference the staged archive
/// `integrity` names instead of carrying it.
pub(crate) fn reference_staged_archive(document: &mut Value, integrity: &str) {
    let Some(attachments) = document.get_mut("_attachments").and_then(Value::as_object_mut) else {
        return;
    };
    for attachment in attachments.values_mut() {
        if let Some(attachment) = attachment.as_object_mut() {
            attachment.remove("data");
            attachment.insert("integrity".to_owned(), Value::String(integrity.to_owned()));
        }
    }
}

/// Stage every archive of `archives` on `registry` that it does not hold
/// staged already, [`CONCURRENT_UPLOADS`] at a time. Returns how many it
/// uploaded.
pub(crate) async fn stage_archives(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archives: &[StagedArchive<'_>],
) -> Result<usize, StagingError> {
    let uploaded = AtomicUsize::new(0);
    stream::iter(archives)
        .map(Ok::<_, StagingError>)
        .try_for_each_concurrent(CONCURRENT_UPLOADS, |archive| {
            let uploaded = &uploaded;
            async move {
                if stage_archive(client, registry, authorization, archive).await? {
                    uploaded.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            }
        })
        .await?;
    Ok(uploaded.into_inner())
}

/// Stage `archive` unless `registry` holds it staged already. Returns
/// whether it was uploaded.
async fn stage_archive(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archive: &StagedArchive<'_>,
) -> Result<bool, StagingError> {
    if staged_headers(client, registry, authorization, &archive.integrity)
        .await?
        .is_some()
    {
        return Ok(false);
    }
    upload_archive(client, registry, authorization, archive).await?;
    Ok(true)
}

/// Upload every archive of `archives` to `registry`'s staging endpoint,
/// without asking whether it is staged already: uploading staged bytes
/// again renews them.
pub(crate) async fn upload_archives(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archives: &[StagedArchive<'_>],
) -> Result<(), StagingError> {
    stream::iter(archives)
        .map(Ok::<_, StagingError>)
        .try_for_each_concurrent(CONCURRENT_UPLOADS, |archive| {
            upload_archive(client, registry, authorization, archive)
        })
        .await
}

/// The size of the archive `registry` holds staged by `integrity` for the
/// caller, or `None` when it holds none, such as one another account staged
/// or one that expired or expires within the hour.
pub(crate) async fn staged_length(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    integrity: &str,
) -> Result<Option<u64>, StagingError> {
    let Some(headers) = staged_headers(client, registry, authorization, integrity).await? else {
        return Ok(None);
    };
    // A `HEAD` response has no body, so its size is the header's, not the
    // body's that `content_length` reports.
    headers
        .get("content-length")
        .and_then(|length| length.to_str().ok()?.parse().ok())
        .map(Some)
        .ok_or_else(|| {
            staging_failure(registry, integrity, "HEAD answered without a Content-Length")
        })
}

/// The headers of `registry`'s answer to `HEAD` for the archive staged by
/// `integrity`, or `None` when it holds none for the caller.
async fn staged_headers(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    integrity: &str,
) -> Result<Option<reqwest::header::HeaderMap>, StagingError> {
    let url = staging_url(registry, integrity)?;
    let guard = client.acquire_for_url(&url).await;
    let mut head = guard.head(&url);
    if let Some(authorization) = authorization {
        head = head.header("authorization", authorization);
    }
    let response = head
        .send()
        .await
        .map_err(|error| staging_failure(registry, integrity, &error.to_string()))?;
    match response.status().as_u16() {
        200 => Ok(Some(response.headers().clone())),
        404 => Ok(None),
        status => Err(staging_failure(registry, integrity, &format!("HEAD answered {status}"))),
    }
}

/// A failure to stage the archive `integrity` on `registry`.
fn staging_failure(
    registry: &NormalizedRegistryUrl,
    integrity: &str,
    reason: &str,
) -> StagingError {
    StagingError::Failed {
        registry: registry.for_display(),
        integrity: integrity.to_string(),
        reason: reason.to_string(),
    }
}

/// Upload `archive` to `registry`'s staging endpoint.
async fn upload_archive(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archive: &StagedArchive<'_>,
) -> Result<(), StagingError> {
    let url = staging_url(registry, &archive.integrity)?;
    let guard = client.acquire_for_url(&url).await;
    let mut put = guard
        .put(&url)
        .header("content-type", "application/octet-stream")
        .header("content-length", archive.bytes.len())
        .body(bytes::Bytes::copy_from_slice(archive.bytes));
    if let Some(authorization) = authorization {
        put = put.header("authorization", authorization);
    }
    let response = put
        .send()
        .await
        .map_err(|error| staging_failure(registry, &archive.integrity, &error.to_string()))?;
    if response.status().is_success() {
        return Ok(());
    }
    Err(staging_failure(registry, &archive.integrity, &refusal(response).await))
}

fn staging_url(registry: &NormalizedRegistryUrl, integrity: &str) -> Result<String, StagingError> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("integrity", integrity)
        .finish();
    registry
        .join(&format!("{STAGING_ENDPOINT}?{query}"))
        .map_err(StagingError::InvalidUrl)
}

/// What a refused upload's `response` says, keeping only the start of a long
/// body.
async fn refusal(response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    let body = match read_limited_body(response, ERROR_BODY_LIMIT).await {
        Ok(body) => {
            let ellipsis = if body.truncated { "…" } else { "" };
            format!("{}{ellipsis}", String::from_utf8_lossy(&body.bytes))
        }
        Err(_) => String::new(),
    };
    format!("PUT answered {status}: {body}")
}

/// An archive could not be staged before the commit that references it.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum StagingError {
    #[display("Uploading the archive {integrity} to {registry} before publishing failed: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_STAGING_FAILED),
        help(
            "A registry declared `serverType: pnpr` receives each archive on its own request before the publish that references it. Nothing was published to this registry. Running the publish again uploads only the archives it still lacks."
        )
    )]
    Failed {
        #[error(not(source))]
        registry: String,
        integrity: String,
        reason: String,
    },

    #[display("invalid registry URL: {_0}")]
    InvalidUrl(url::ParseError),
}

#[cfg(test)]
mod tests;
