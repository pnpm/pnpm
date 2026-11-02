//! Writing an attachment into its blob slot, and deriving the tar a `.tgz`
//! decodes to while a decode permit is held.

use super::{
    super::{
        stream_decode_verify_and_write, tar_integrity_and_manifest_of, tgz_integrity_key,
        write_sigstore_bundle,
    },
    AttachmentKind, Integrity, PendingAttachment, RegistryError, Value,
};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Where an attachment is written, and where its tar is encoded to when the
/// registry stores tars.
pub(super) struct AttachmentPaths {
    attachment: std::path::PathBuf,
    tar: Option<std::path::PathBuf>,
}

impl AttachmentPaths {
    /// The paths for the blob slot at `slot`. With `stores_tar`, the verified
    /// `.tgz` waits beside the slot until its tar is encoded into it.
    pub(super) fn for_slot(slot: &std::path::Path, stores_tar: bool) -> Self {
        if !stores_tar {
            return Self { attachment: slot.to_path_buf(), tar: None };
        }
        let mut tgz = slot.as_os_str().to_os_string();
        tgz.push(".tgz");
        Self { attachment: tgz.into(), tar: Some(slot.to_path_buf()) }
    }
}

/// Run [`write_and_derive_tar`] on a blocking thread. A tarball waits for a
/// permit from `decodes` first.
pub(super) async fn write_under_permit(
    attachment: PendingAttachment,
    kind: AttachmentKind,
    dist: Value,
    paths: AttachmentPaths,
    decodes: &Arc<Semaphore>,
) -> Result<Result<Option<DerivedTgz>, RegistryError>, tokio::task::JoinError> {
    let permit = match kind {
        AttachmentKind::Tarball | AttachmentKind::Variant => Some(
            Arc::clone(decodes)
                .acquire_owned()
                .await
                .expect("the decode semaphore is never closed"),
        ),
        AttachmentKind::Sigstore => None,
    };
    tokio::task::spawn_blocking(move || {
        // The permit covers the encode too: it decodes the `.tgz` again.
        let derived =
            write_and_derive_tar(&attachment, kind, &dist, &paths.attachment, permit.as_ref());
        let Some(tar_path) = &paths.tar else { return derived };
        let stored = derived.and_then(|derived| {
            let derived = derived.ok_or_else(|| RegistryError::BadRequest {
                reason: format!("{} does not decode to a tar", attachment.filename),
            })?;
            encode_stored_tar(&paths.attachment, tar_path)?;
            Ok(Some(derived))
        });
        let _ = std::fs::remove_file(&paths.attachment);
        stored
    })
    .await
}

/// What a published `.tgz` derives.
pub(super) struct DerivedTgz {
    /// The `.tgz` integrity, as the tar-integrity table keys it.
    pub(super) tgz_integrity: String,
    /// The integrity of the tar it decodes to.
    pub(super) integrity: Integrity,
    pub(super) inspection: pnpm_tarball::TarInspection,
    /// Its root `package.json`, read as extraction reads it.
    pub(super) manifest: Option<Value>,
}

/// Write the attachment to `tmp_path`, and for a `.tgz` return what it
/// derives. `_decode` is held until the tar is hashed and inspected.
fn write_and_derive_tar(
    attachment: &PendingAttachment,
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
    _decode: Option<&OwnedSemaphorePermit>,
) -> Result<Option<DerivedTgz>, RegistryError> {
    decode_and_write_attachment(attachment, kind, dist, tmp_path)?;
    if kind == AttachmentKind::Sigstore {
        return Ok(None);
    }
    let Some(tgz_integrity) = dist.as_object().and_then(tgz_integrity_key) else {
        return Ok(None);
    };
    let tgz = std::fs::File::open(tmp_path)?;
    Ok(tar_integrity_and_manifest_of(std::io::BufReader::new(tgz))
        .map(|(integrity, inspection, manifest)| DerivedTgz {
            tgz_integrity,
            integrity,
            inspection,
            manifest,
        }))
}

/// Compress the tar the verified `.tgz` at `tgz_path` decodes to into
/// `tar_path`. [`tar_integrity_and_manifest_of`] already decoded it within
/// the limit.
fn encode_stored_tar(
    tgz_path: &std::path::Path,
    tar_path: &std::path::Path,
) -> Result<(), RegistryError> {
    let tgz = std::io::BufReader::new(std::fs::File::open(tgz_path)?);
    let tar = flate2::read::MultiGzDecoder::new(tgz);
    let out = std::io::BufWriter::new(std::fs::File::create(tar_path)?);
    super::super::super::tar_variants::encode_tar(tar, out, None)?
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?
        .sync_all()?;
    Ok(())
}

fn decode_and_write_attachment(
    attachment: &PendingAttachment,
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
) -> Result<(), RegistryError> {
    let dist_ref = (!dist.is_null()).then_some(dist);
    match kind {
        AttachmentKind::Tarball | AttachmentKind::Variant => {
            stream_decode_verify_and_write(
                &attachment.filename,
                &attachment.data,
                attachment.declared_length,
                dist_ref,
                tmp_path,
            )?;
        }
        AttachmentKind::Sigstore => {
            write_sigstore_bundle(
                &attachment.filename,
                &attachment.data,
                attachment.declared_length,
                tmp_path,
            )?;
        }
    }
    Ok(())
}
