//! The attachments of a publish body: each one's bytes, carried inline or
//! staged before the publish, and the length it declares.

use pnpr_error::{RegistryError, Result};
use serde_json::Value;

/// Per-tarball metadata pulled out of an `_attachments` entry.
#[derive(Debug)]
pub struct PendingAttachment {
    pub filename: String,
    pub payload: AttachmentPayload,
    /// `_attachments.<filename>.length` — the declared byte count of
    /// the *decoded* tarball, used to catch truncated uploads.
    pub declared_length: Option<u64>,
    /// `_attachments.<filename>.integrity`: the integrity of a staged
    /// archive, or the one inline bytes declare beside `data`.
    pub declared_integrity: Option<String>,
}

/// Where an attachment's bytes come from.
#[derive(Debug)]
pub enum AttachmentPayload {
    /// Base64-encoded, taken verbatim from the publish body. We hold it as
    /// an owned `String` rather than decoding it eagerly — the streaming
    /// write path consumes it directly and the decoded bytes never have to
    /// live in memory.
    Inline(String),
    /// An archive the publisher staged by this integrity before the commit,
    /// named by `_attachments.<filename>.integrity` without `data`.
    Staged { integrity: String },
}

impl PendingAttachment {
    /// The base64 payload the publish body carried, or `None` for a staged
    /// archive.
    #[must_use]
    pub fn inline_data(&self) -> Option<&str> {
        match &self.payload {
            AttachmentPayload::Inline(data) => Some(data),
            AttachmentPayload::Staged { .. } => None,
        }
    }

    /// The integrity of the staged archive this attachment references.
    #[must_use]
    pub fn staged_integrity(&self) -> Option<&str> {
        match &self.payload {
            AttachmentPayload::Inline(_) => None,
            AttachmentPayload::Staged { integrity } => Some(integrity),
        }
    }
}

/// Pull all `_attachments` out of a publish body. Returns one
/// [`PendingAttachment`] per entry and removes the `_attachments`
/// field from `body` so the saved packument doesn't duplicate the
/// on-disk tarball. Base64 decoding is deferred to the streaming
/// write path; this function just validates shape.
pub fn extract_attachments(body: &mut Value) -> Result<Vec<PendingAttachment>, RegistryError> {
    let Some(obj) = body.as_object_mut() else {
        return Ok(Vec::new());
    };
    let Some(attachments_value) = obj.remove("_attachments") else {
        return Ok(Vec::new());
    };
    let Value::Object(attachments) = attachments_value else {
        return Err(RegistryError::BadRequest {
            reason: "_attachments must be an object".to_string(),
        });
    };
    let mut out = Vec::with_capacity(attachments.len());
    for (filename, value) in attachments {
        let Value::Object(mut value_obj) = value else {
            return Err(RegistryError::InvalidAttachment {
                filename,
                reason: "expected object".to_string(),
            });
        };
        let Some((payload, declared_integrity)) = attachment_payload(&mut value_obj) else {
            return Err(RegistryError::InvalidAttachment {
                filename,
                reason: "needs a string `data`, a string `integrity` naming a staged archive, \
                         or both"
                    .to_string(),
            });
        };
        let declared_length = value_obj.get("length").and_then(Value::as_u64);
        out.push(PendingAttachment { filename, payload, declared_length, declared_integrity });
    }
    Ok(out)
}

/// Where an attachment's bytes come from, and the integrity it declares, or
/// `None` when it names neither inline bytes nor a staged archive.
fn attachment_payload(
    attachment: &mut serde_json::Map<String, Value>,
) -> Option<(AttachmentPayload, Option<String>)> {
    match (attachment.remove("data"), attachment.remove("integrity")) {
        (Some(Value::String(data)), None) => Some((AttachmentPayload::Inline(data), None)),
        (Some(Value::String(data)), Some(Value::String(integrity))) => {
            Some((AttachmentPayload::Inline(data), Some(integrity)))
        }
        (None, Some(Value::String(integrity))) => {
            Some((AttachmentPayload::Staged { integrity: integrity.clone() }, Some(integrity)))
        }
        _ => None,
    }
}
