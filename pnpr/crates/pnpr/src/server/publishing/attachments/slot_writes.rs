//! Writing an attachment into its blob slot, and deriving what a `.tgz`
//! decodes to while a decode permit is held.

use super::{
    super::{
        stream_decode_verify_and_write, tar_integrity_of, tgz_integrity_key,
        verification::read_archive, verify_and_write, write_sigstore_bundle,
    },
    AttachmentKind, PendingAttachment, RegistryError, TarDerivation, Value,
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
/// permit from `decodes` first, unless the caller `held` one already.
/// `staged` holds the bytes of the staged archive the attachment
/// references, when it references one.
pub(super) async fn write_under_permit(
    (attachment, staged): (PendingAttachment, Option<Vec<u8>>),
    kind: AttachmentKind,
    dist: Value,
    paths: AttachmentPaths,
    (decodes, held): (&Arc<Semaphore>, Option<OwnedSemaphorePermit>),
) -> Result<Result<Option<TarDerivation>, RegistryError>, tokio::task::JoinError> {
    let permit = match (kind, held) {
        (AttachmentKind::Tarball | AttachmentKind::Variant, Some(permit)) => Some(permit),
        (AttachmentKind::Tarball | AttachmentKind::Variant, None) => Some(
            Arc::clone(decodes)
                .acquire_owned()
                .await
                .expect("the decode semaphore is never closed"),
        ),
        (AttachmentKind::Sigstore, _) => None,
    };
    tokio::task::spawn_blocking(move || {
        // The permit covers the encode too: it decodes the `.tgz` again.
        let derived = write_and_derive_tar(
            (&attachment, staged),
            kind,
            &dist,
            &paths.attachment,
            permit.as_ref(),
        );
        let Some(tar_path) = &paths.tar else { return derived };
        let stored = derived.and_then(|derived| {
            let derived = derived
                .filter(|derived| derived.tgz_integrity.is_some())
                .ok_or_else(|| RegistryError::BadRequest {
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

/// Write the attachment to `tmp_path`, and for a `.tgz` derive what it
/// decodes to. A `.tgz` that does not decode in full to a tar, or whose tar
/// does not name its manifest unambiguously, fails with a 400: its manifest
/// cannot be verified. `_decode` is held until the tar is read.
fn write_and_derive_tar(
    (attachment, staged): (&PendingAttachment, Option<Vec<u8>>),
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
    _decode: Option<&OwnedSemaphorePermit>,
) -> Result<Option<TarDerivation>, RegistryError> {
    decode_and_write_attachment((attachment, staged), kind, dist, tmp_path)?;
    if kind == AttachmentKind::Sigstore {
        return Ok(None);
    }
    let tgz = std::fs::File::open(tmp_path)?;
    let (integrity, inspection) = tar_integrity_of(std::io::BufReader::new(tgz))
        .ok_or_else(|| RegistryError::InvalidAttachment {
            filename: attachment.filename.clone(),
            reason: "it does not decode to a tar within the decode limit".to_string(),
        })?;
    let archive = read_archive(tmp_path, &attachment.filename)?;
    let tgz_integrity = dist.as_object().and_then(tgz_integrity_key);
    Ok(Some(TarDerivation { tgz_integrity, integrity, inspection, archive }))
}

/// Compress the tar the verified `.tgz` at `tgz_path` decodes to into
/// `tar_path`. [`tar_integrity_of`] already decoded it within the limit.
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
    (attachment, staged): (&PendingAttachment, Option<Vec<u8>>),
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
) -> Result<(), RegistryError> {
    let dist_ref = (!dist.is_null()).then_some(dist);
    let filename = &attachment.filename;
    let length = attachment.declared_length;
    match (kind, attachment.inline_data(), staged) {
        (AttachmentKind::Tarball | AttachmentKind::Variant, Some(data), _) => {
            stream_decode_verify_and_write(filename, data, length, dist_ref, tmp_path)?;
        }
        (AttachmentKind::Tarball | AttachmentKind::Variant, None, Some(bytes)) => {
            verify_and_write(filename, bytes.as_slice(), length, dist_ref, tmp_path)?;
        }
        (AttachmentKind::Sigstore, Some(data), _) => {
            write_sigstore_bundle(filename, data, length, tmp_path)?;
        }
        // Refused while preparing the attachment, and fetched before this.
        (_, None, _) => {
            return Err(RegistryError::Internal {
                reason: format!("{filename} references a staged archive that was not read"),
            });
        }
    }
    Ok(())
}
