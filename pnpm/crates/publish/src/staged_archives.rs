//! Uploading a batch's archives to pnpr before the commit that references
//! them, so the commit carries each archive's integrity instead of its
//! bytes.
//!
//! pnpr stages an archive uploaded to
//! `PUT <registry>/-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` for the
//! account that uploaded it, and answers `HEAD` on the same URL when it is
//! staged already, so a resumed publish uploads only what is missing.

use futures_util::future::try_join_all;
use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network::ThrottledClient;
use serde_json::Value;

use crate::{
    publish_packed_pkg::{PublishPackedPkgError, join_registry, registry_for_display},
    registry_config_keys::NormalizedRegistryUrl,
};

const STAGING_ENDPOINT: &str = "-/pnpr/v0/artifacts/blob";

/// One archive to stage: its bytes and their SRI SHA-512.
pub(crate) struct StagedArchive {
    pub(crate) integrity: String,
    pub(crate) bytes: bytes::Bytes,
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
/// staged already, all at once: the shared client's request slots bound how
/// many are in flight.
pub(crate) async fn stage_archives(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archives: &[StagedArchive],
) -> Result<(), StagingError> {
    let uploads =
        archives.iter().map(|archive| stage_archive(client, registry, authorization, archive));
    try_join_all(uploads).await?;
    Ok(())
}

async fn stage_archive(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    archive: &StagedArchive,
) -> Result<(), StagingError> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("integrity", &archive.integrity)
        .finish();
    let url = join_registry(registry, &format!("{STAGING_ENDPOINT}?{query}"))?;
    let failed = |reason: String| StagingError::Failed {
        registry: registry_for_display(registry),
        integrity: archive.integrity.clone(),
        reason,
    };

    let guard = client.acquire_for_url(&url).await;
    let mut head = guard.head(&url);
    if let Some(authorization) = authorization {
        head = head.header("authorization", authorization);
    }
    let response = head.send().await.map_err(|error| failed(error.to_string()))?;
    match response.status().as_u16() {
        200 => return Ok(()),
        404 => {}
        status => return Err(failed(format!("HEAD answered {status}"))),
    }
    drop(guard);

    let guard = client.acquire_for_url(&url).await;
    let mut put = guard
        .put(&url)
        .header("content-type", "application/octet-stream")
        .header("content-length", archive.bytes.len())
        .body(archive.bytes.clone());
    if let Some(authorization) = authorization {
        put = put.header("authorization", authorization);
    }
    let response = put.send().await.map_err(|error| failed(error.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(failed(format!("PUT answered {}: {body}", status.as_u16())))
}

/// An archive could not be staged before the commit that references it.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum StagingError {
    #[display("Uploading the archive {integrity} to {registry} before publishing failed: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_STAGING_FAILED),
        help(
            "A registry declared `serverType: pnpr` receives each archive on its own request before the batch commit. Nothing was published; run the publish again to retry."
        )
    )]
    Failed {
        #[error(not(source))]
        registry: String,
        integrity: String,
        reason: String,
    },

    #[diagnostic(transparent)]
    Url(PublishPackedPkgError),
}

impl From<PublishPackedPkgError> for StagingError {
    fn from(error: PublishPackedPkgError) -> Self {
        StagingError::Url(error)
    }
}

#[cfg(test)]
mod tests;
