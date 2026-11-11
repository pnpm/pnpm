//! Writing an attachment into its blob slot, and deriving what a `.tgz`
//! decodes to while a decode permit is held.

use super::{
    super::{
        stream_decode_verify_and_write, tar_integrity_of, tgz_integrity_key,
        verification::read_archive, verify_and_write, write_sigstore_bundle,
    },
    AttachmentKind, CanonicalPackageName, PendingAttachment, PreparedAttachment, PublishedTar,
    RegistryError, Storage, TarDerivation, TarballStorage, Value, WrittenAttachments,
    cleanup_tmp_slots, variants,
};
use crate::server::staged_archives::claim_staged_archive;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Write every attachment into its reserved blob slot. A failure removes
/// the slots written so far, so a bad upload leaves no on-disk artifact.
///
/// `storage` comes with the principal publishing, whose staged archives an
/// attachment may reference.
pub(in super::super) async fn write_attachment_slots(
    (storage, claimant): (&Storage, Option<&str>),
    name: &CanonicalPackageName,
    prepared: Vec<PreparedAttachment>,
    tarballs: TarballStorage,
    decodes: &Arc<Semaphore>,
) -> Result<WrittenAttachments, RegistryError> {
    let mut written = WrittenAttachments {
        slots: Vec::with_capacity(prepared.len()),
        tars: Vec::new(),
        variants: Vec::new(),
    };
    for attachment in prepared {
        match write_attachment_slot((storage, claimant), name, attachment, tarballs, decodes).await
        {
            Ok((slot, tar)) => {
                written.slots.push(slot);
                match tar.map(variants::split_variant) {
                    Some(Ok(variant)) => written.variants.push(variant),
                    Some(Err(tar)) => written.tars.push(tar),
                    None => {}
                }
            }
            Err(err) => {
                cleanup_tmp_slots(written.slots).await;
                return Err(err);
            }
        }
    }
    Ok(written)
}

/// Stream-decode, verify and write one tarball into a reserved slot, and
/// derive the tar a `.tgz` decodes to and the manifest inputs it holds. A
/// mismatch, a missing integrity field, or a `.tgz` that does not decode in
/// full to a readable package archive fails with a 400. Under
/// [`TarballStorage::Tar`] the slot holds the tar. A tarball holds a permit
/// from `decodes` until its tar is read.
pub(in super::super) async fn write_attachment_slot(
    (storage, claimant): (&Storage, Option<&str>),
    name: &CanonicalPackageName,
    prepared: PreparedAttachment,
    tarballs: TarballStorage,
    decodes: &Arc<Semaphore>,
) -> Result<(pnpr_storage::BlobSlot, Option<PublishedTar>), RegistryError> {
    // A staged archive is read before a slot is reserved: one that is missing
    // or expired fails the publish with nothing to clean up.
    let (staged, permit) = staged_bytes((storage, claimant), &prepared.attachment, decodes).await?;
    let slot_storage = match (tarballs, prepared.kind) {
        (TarballStorage::Tar, AttachmentKind::Tarball) => TarballStorage::Tar,
        _ => TarballStorage::Tgz,
    };
    let blob = match slot_storage {
        TarballStorage::Tar => name.tar_name_for_version(&prepared.version),
        TarballStorage::Tgz => prepared.canonical.clone(),
    };
    let slot = storage.reserve_hosted_blob(name, &blob).await?.for_version(&prepared.version);
    let (slot, derived) = write_under_permit(
        slot,
        slot_storage,
        (prepared.attachment, staged),
        prepared.kind,
        prepared.dist,
        (decodes, permit),
    )
    .await?;
    let Some(derived) = derived else { return Ok((slot, None)) };
    if let Some(tgz_integrity) = &derived.tgz_integrity {
        let recorded = record_published_tar(storage, name, tgz_integrity, &derived, slot_storage);
        if let Err(err) = recorded.await {
            slot.abandon().await;
            return Err(err);
        }
    }
    Ok((slot, Some(published_tar(prepared.version, prepared.variant_tag, derived))))
}

async fn staged_bytes(
    (storage, claimant): (&Storage, Option<&str>),
    attachment: &PendingAttachment,
    decodes: &Arc<Semaphore>,
) -> Result<(Option<Vec<u8>>, Option<OwnedSemaphorePermit>), RegistryError> {
    let Some(integrity) = attachment.staged_integrity() else { return Ok((None, None)) };
    let permit =
        Arc::clone(decodes).acquire_owned().await.expect("the decode semaphore is never closed");
    let bytes = claim_staged_archive(storage, claimant, &attachment.filename, integrity).await?;
    Ok((Some(bytes), Some(permit)))
}

fn published_tar(
    version: String,
    variant_tag: Option<String>,
    derived: TarDerivation,
) -> PublishedTar {
    let variant = variant_tag.map(|tag| {
        let tgz = derived.tgz_integrity.as_deref();
        variants::variant_tar(tag, &derived.integrity, tgz, &derived.archive)
    });
    let TarDerivation {
        tgz_integrity,
        integrity,
        inspection,
        archive,
    } = derived;
    PublishedTar {
        version,
        tgz_integrity,
        integrity,
        inspection,
        archive,
        variant,
        generated: false,
    }
}

async fn record_published_tar(
    storage: &Storage,
    name: &CanonicalPackageName,
    tgz_integrity: &str,
    derived: &TarDerivation,
    slot_storage: TarballStorage,
) -> Result<(), RegistryError> {
    let inspection = &derived.inspection;
    let derived = pnpr_storage::DerivedTar::new(
        derived.integrity.to_string(),
        inspection.requires_build,
        inspection.file_count,
        inspection.unpacked_size,
    )
    .with_bins(inspection.bins.clone());
    let Err(err) = storage.record_tar_integrity(name, tgz_integrity, derived).await else {
        return Ok(());
    };
    tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
    match slot_storage {
        TarballStorage::Tar => Err(err),
        TarballStorage::Tgz => Ok(()),
    }
}

/// Where an attachment is written, and where its tar is encoded to when the
/// registry stores tars.
pub(super) struct AttachmentPaths {
    attachment: std::path::PathBuf,
    tar: Option<std::path::PathBuf>,
}

impl AttachmentPaths {
    /// The paths for the blob slot at `slot`. For [`TarballStorage::Tar`], the
    /// verified `.tgz` waits beside the slot until its tar is encoded into it.
    pub(super) fn for_slot(slot: &std::path::Path, storage: TarballStorage) -> Self {
        match storage {
            TarballStorage::Tgz => Self { attachment: slot.to_path_buf(), tar: None },
            TarballStorage::Tar => Self {
                attachment: pnpr_storage::unique_tmp_path(&slot.with_file_name("archive")),
                tar: Some(slot.to_path_buf()),
            },
        }
    }
}

/// Run [`write_and_derive_tar`] on a blocking thread. Moving `slot` into the
/// blocking task ensures its temporary file is removed after all writes close,
/// even if the caller drops the future on timeout or disconnect.
pub(super) async fn write_under_permit(
    slot: pnpr_storage::BlobSlot,
    storage: TarballStorage,
    (attachment, staged): (PendingAttachment, Option<Vec<u8>>),
    kind: AttachmentKind,
    dist: Value,
    (decodes, held): (&Arc<Semaphore>, Option<OwnedSemaphorePermit>),
) -> Result<(pnpr_storage::BlobSlot, Option<TarDerivation>), RegistryError> {
    let permit = acquire_decode_permit(kind, decodes, held).await;
    tokio::task::spawn_blocking(move || {
        let paths = AttachmentPaths::for_slot(&slot.tmp_path, storage);
        let extra =
            paths.tar.is_some().then(|| pnpr_storage::TempFileGuard::new(paths.attachment.clone()));
        let derived = run_slot_write((attachment, staged), kind, &dist, &paths, permit.as_ref())?;
        drop(extra);
        Ok((slot, derived))
    })
    .await
    .map_err(|join_err| RegistryError::Io(std::io::Error::other(join_err.to_string())))?
}

async fn acquire_decode_permit(
    kind: AttachmentKind,
    decodes: &Arc<Semaphore>,
    held: Option<OwnedSemaphorePermit>,
) -> Option<OwnedSemaphorePermit> {
    match (kind, held) {
        (AttachmentKind::Tarball | AttachmentKind::Variant, Some(permit)) => Some(permit),
        (AttachmentKind::Tarball | AttachmentKind::Variant, None) => Some(
            Arc::clone(decodes)
                .acquire_owned()
                .await
                .expect("the decode semaphore is never closed"),
        ),
        (AttachmentKind::Sigstore, _) => None,
    }
}

fn run_slot_write(
    (attachment, staged): (PendingAttachment, Option<Vec<u8>>),
    kind: AttachmentKind,
    dist: &Value,
    paths: &AttachmentPaths,
    permit: Option<&OwnedSemaphorePermit>,
) -> Result<Option<TarDerivation>, RegistryError> {
    // The permit covers the encode too: it decodes the `.tgz` again.
    let derived =
        write_and_derive_tar((&attachment, staged), kind, dist, &paths.attachment, permit)?;
    let Some(tar_path) = &paths.tar else { return Ok(derived) };
    let derived = derived
        .filter(|derived| derived.tgz_integrity.is_some())
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!("{} does not decode to a tar", attachment.filename),
        })?;
    encode_stored_tar(&paths.attachment, tar_path)?;
    Ok(Some(derived))
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

#[cfg(test)]
mod tests;
