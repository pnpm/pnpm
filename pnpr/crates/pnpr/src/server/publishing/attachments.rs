use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentVersion, Identity,
    Integrity, PendingAttachment, RegistryError, RegistrySource, Storage, Value, WriteTarget,
    authorize_package, cleanup_tmp_slots, extract_attachments, hosted_storage,
    manifest::{
        check_publishable_versions, parse_hosted_packument, prepare_staged_document,
        record_publisher, restore_submitted_tgz_integrities, serialize_staged_document,
    },
    resolve_write_target, stream_decode_verify_and_write, tar_integrity_of, tgz_integrity_key,
    tombstones::{check_publish_tombstone, check_staged_tombstone},
    variants::{self, Declarations, PublishedVariant, VariantTar},
    write_sigstore_bundle,
};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// What a hosted registry keeps of a published `.tgz`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum TarballStorage {
    /// The `.tgz` as published.
    Tgz,
    /// Only the tar it decodes to, compressed at level 19 (see
    /// [`CanonicalPackageName::tar_name_for_version`]): a registry that
    /// declares `tarballs: tar`.
    Tar,
}

/// What kind of payload an attachment carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum AttachmentKind {
    Tarball,
    Sigstore,
    /// A platform variant's `.tgz` (see [`variants`]).
    Variant,
}

/// A publish document that passed every check that can run before
/// taking the package lock: the caller may publish the package, and
/// each attachment maps to a canonical disk filename and a
/// `versions[v].dist` block.
pub(in super::super) struct ValidatedPublish {
    pub(in super::super) name: CanonicalPackageName,
    /// The publish body with `_attachments` stripped.
    pub(in super::super) incoming: Value,
    /// One entry per attachment.
    pub(in super::super) prepared: Vec<PreparedAttachment>,
    /// The caller these checks admitted, for the checks that repeat under
    /// the package lock.
    pub(in super::super) publisher: Identity,
    pub(in super::super) tarballs: TarballStorage,
    /// Declared platform variants, by version.
    pub(in super::super) variants: Declarations,
}

/// One publish attachment resolved to its canonical on-disk filename and its
/// `versions[version].dist` block.
pub(in super::super) struct PreparedAttachment {
    pub(super) attachment: PendingAttachment,
    /// Canonical on-disk filename.
    pub(super) canonical: String,
    /// The version this attachment publishes, parsed from its filename.
    /// Lets the re-publish guard tell a content publish from a metadata-only
    /// update (which carries no attachments).
    pub(in super::super) version: String,
    /// The matching `dist` block, or `Value::Null` when absent.
    pub(in super::super) dist: Value,
    pub(in super::super) kind: AttachmentKind,
    /// The variant tag of an [`AttachmentKind::Variant`].
    pub(in super::super) variant_tag: Option<String>,
}

pub(in super::super) async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: CanonicalPackageName,
    mut incoming: Value,
) -> Result<(ValidatedPublish, WriteTarget), RegistryError> {
    // Route write to hosted target and enforce the registry's publish rule.
    let target = resolve_write_target(state, identity, registry, &name)?;
    authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await?;

    let storage = hosted_storage(state, Some(&target.org));
    check_publish_tombstone(state, identity, &storage, &name).await?;

    let attachments = extract_attachments(&mut incoming)?;
    record_publisher(&mut incoming, identity);
    let variants = variants::take_declarations(&mut incoming)?;

    // Validate attachment filenames against the package and parse their
    // canonical storage filename, matching version, and payload kind.
    let prepared = prepare_attachments(&name, &incoming, attachments, &variants)?;
    let publisher = identity.clone();
    let tarballs = if super::super::tar_registry::is_tar_registry(state, &target.source) {
        TarballStorage::Tar
    } else {
        TarballStorage::Tgz
    };
    Ok((ValidatedPublish { name, incoming, prepared, publisher, tarballs, variants }, target))
}

fn prepare_attachments(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachments: Vec<PendingAttachment>,
    declared_variants: &Declarations,
) -> Result<Vec<PreparedAttachment>, RegistryError> {
    let mut prepared = Vec::with_capacity(attachments.len());
    let mut seen_canonical = HashSet::with_capacity(attachments.len());
    let mut tarball_versions = HashSet::new();

    for attachment in attachments {
        let item = prepare_attachment(name, incoming, attachment, declared_variants)?;
        if !seen_canonical.insert(item.canonical.clone()) {
            return Err(RegistryError::BadRequest {
                reason: format!("duplicate attachment for {}", item.canonical),
            });
        }
        if item.kind == AttachmentKind::Tarball {
            tarball_versions.insert(item.version.clone());
        }
        prepared.push(item);
    }
    for att in &prepared {
        if att.kind == AttachmentKind::Sigstore && !tarball_versions.contains(&att.version) {
            return Err(RegistryError::BadRequest {
                reason: format!("sigstore attachment for {} has no tarball", att.version),
            });
        }
    }
    Ok(prepared)
}

/// Prepare an attachment to be verified against its version's `dist`, or a
/// variant's against its declaration.
fn prepare_attachment(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachment: PendingAttachment,
    declared_variants: &Declarations,
) -> Result<PreparedAttachment, RegistryError> {
    if let Some(parsed) = variants::parse_variant_attachment(name, &attachment.filename) {
        return variants::prepare_variant_attachment(attachment, parsed?, declared_variants);
    }
    let (canonical, version, kind) = parse_attachment_kind(name, &attachment.filename)?;
    let dist = incoming
        .get("versions")
        .and_then(|versions| versions.get(&version))
        .and_then(|manifest| manifest.get("dist"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(PreparedAttachment { attachment, canonical, version, dist, kind, variant_tag: None })
}

fn parse_attachment_kind(
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(String, String, AttachmentKind), RegistryError> {
    if filename.ends_with(".tgz") {
        let (canonical, version) = name.parse_tarball_name(filename)?;
        Ok((canonical, version, AttachmentKind::Tarball))
    } else if filename.ends_with(".sigstore") || filename.ends_with(".sigstore.json") {
        let (canonical, version) = name.parse_sigstore_name(filename)?;
        Ok((canonical, version, AttachmentKind::Sigstore))
    } else {
        Err(RegistryError::InvalidTarballName {
            package: name.as_str().to_string(),
            filename: filename.to_string(),
        })
    }
}

/// A publish whose document is computed and whose blobs are fully written to
/// tmp slots — everything verified, nothing visible to readers yet.
/// [`commit_publishes`](super::commit_publishes) makes it visible. Every surface stages into this, so
/// one commit can carry packages of more than one ecosystem.
pub(in super::super) struct StagedPublish {
    pub(in super::super) name: CanonicalPackageName,
    pub(in super::super) ecosystem: Ecosystem,
    /// The document to record, merged with what the store held when it was
    /// read: a packument, a crate document, a project document.
    pub(in super::super) document: Vec<u8>,
    pub(in super::super) base_version: Option<HostedDocumentVersion>,
    pub(in super::super) slots: Vec<pnpr_storage::BlobSlot>,
    /// Hosted-org storage namespace this publish targets, or `None` for the
    /// flat (path-less) hosted store. Threaded into the commit and journal so
    /// the write — and any crash recovery — lands in the right org.
    pub(in super::super) org: Option<String>,
}

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`](super::commit_publishes). On error, every tmp file this call wrote is
/// removed.
pub(in super::super) async fn stage_publish(
    state: &AppState,
    doc: ValidatedPublish,
    now_iso: &str,
    org: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let storage = hosted_storage(state, org);

    let (hosted, base_version) = parse_hosted_packument(
        &doc.name,
        storage.read_hosted_document_for_update(&doc.name).await?,
    )?;

    check_staged_tombstone(state, &storage, &doc, hosted.is_some()).await?;
    let ValidatedPublish {
        name,
        mut incoming,
        prepared,
        tarballs,
        variants,
        ..
    } = doc;
    restore_submitted_tgz_integrities(&storage, &name, hosted.as_ref(), &mut incoming).await?;

    check_publishable_versions(&name, &incoming, hosted.as_ref(), &prepared)?;

    let mut merged = prepare_staged_document(&name, hosted.as_ref(), &incoming, &prepared, now_iso);
    // `incoming` is no longer needed; drop it so the base64 strings
    // inside go away as soon as `prepared` (which owns each one) is
    // drained below.
    drop(incoming);

    // Stream-decode + verify + write each tarball. A mismatch — or a
    // missing integrity field — short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let written =
        write_attachment_slots(&storage, &name, prepared, tarballs, &state.inner.proxy.tar_decodes)
            .await?;
    let written = variants::attach_written_variants(&mut merged, &variants, written).await?;
    let document = serialize_staged_document(merged, &written.tars);
    Ok(StagedPublish {
        name,
        ecosystem: Ecosystem::Npm,
        document,
        base_version,
        slots: written.slots,
        org: org.map(str::to_string),
    })
}

/// The blob slots a publish wrote, and the tar each of its `.tgz` files
/// decodes to.
pub(super) struct WrittenAttachments {
    pub(super) slots: Vec<pnpr_storage::BlobSlot>,
    pub(super) tars: Vec<PublishedTar>,
    pub(super) variants: Vec<PublishedVariant>,
}

/// A published `.tgz` whose whole gzip stream decoded to a tar.
pub(super) struct PublishedTar {
    pub(super) version: String,
    pub(super) inspection: pnpm_tarball::TarInspection,
    /// Set for a platform variant's `.tgz`.
    pub(super) variant: Option<VariantTar>,
}

/// Write every attachment into its reserved blob slot. A failure removes
/// the slots written so far, so a bad upload leaves no on-disk artifact.
pub(super) async fn write_attachment_slots(
    storage: &Storage,
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
        match write_attachment_slot(storage, name, attachment, tarballs, decodes).await {
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
/// derive the tar a `.tgz` decodes to. A mismatch, or a missing integrity
/// field, fails with a 400. A `.tgz` that does not decode in full has no tar
/// and is published without one, except under [`TarballStorage::Tar`], which
/// stores the tar in the slot and fails such a `.tgz` with a 400. A tarball
/// holds a permit from `decodes` until its tar is hashed.
pub(super) async fn write_attachment_slot(
    storage: &Storage,
    name: &CanonicalPackageName,
    prepared: PreparedAttachment,
    tarballs: TarballStorage,
    decodes: &Arc<Semaphore>,
) -> Result<(pnpr_storage::BlobSlot, Option<PublishedTar>), RegistryError> {
    let stores_tar = tarballs == TarballStorage::Tar && prepared.kind == AttachmentKind::Tarball;
    let blob = if stores_tar {
        name.tar_name_for_version(&prepared.version)
    } else {
        prepared.canonical.clone()
    };
    let slot = storage.reserve_hosted_blob(name, &blob).await?;
    let paths = AttachmentPaths::for_slot(&slot.tmp_path, stores_tar);
    let result =
        write_under_permit(prepared.attachment, prepared.kind, prepared.dist, paths, decodes).await;
    let derived = match result {
        Ok(Ok(derived)) => derived,
        Ok(Err(err)) => {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            return Err(err);
        }
        Err(join_err) => {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            return Err(RegistryError::Io(std::io::Error::other(join_err.to_string())));
        }
    };
    let Some((tgz_integrity, integrity, inspection)) = derived else {
        let slot = variants::keep_undecoded(slot, prepared.kind, &prepared.canonical).await?;
        return Ok((slot, None));
    };
    if let Err(err) =
        record_published_tar(storage, name, &tgz_integrity, &integrity, &inspection, stores_tar)
            .await
    {
        let _ = tokio::fs::remove_file(&slot.tmp_path).await;
        return Err(err);
    }
    let variant = variants::variant_tar(prepared.variant_tag, &integrity, &slot.tmp_path);
    Ok((slot, Some(PublishedTar { version: prepared.version, inspection, variant })))
}

/// Record the tar a published `.tgz` decodes to. A failure only warns: a tar
/// registry's first tar response records it later. A stored tar is served
/// without its `.tgz`, so with `stores_tar` nothing would, and the failure
/// fails the publish.
async fn record_published_tar(
    storage: &Storage,
    name: &CanonicalPackageName,
    tgz_integrity: &str,
    integrity: &Integrity,
    inspection: &pnpm_tarball::TarInspection,
    stores_tar: bool,
) -> Result<(), RegistryError> {
    let derived = pnpr_storage::DerivedTar::new(
        integrity.to_string(),
        inspection.requires_build,
        inspection.file_count,
        inspection.unpacked_size,
    )
    .with_bins(inspection.bins.clone());
    let Err(err) = storage.record_tar_integrity(name, tgz_integrity, derived).await else {
        return Ok(());
    };
    tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
    if stores_tar { Err(err) } else { Ok(()) }
}

/// Where an attachment is written, and where its tar is encoded to when the
/// registry stores tars.
struct AttachmentPaths {
    attachment: std::path::PathBuf,
    tar: Option<std::path::PathBuf>,
}

impl AttachmentPaths {
    /// The paths for the blob slot at `slot`. With `stores_tar`, the verified
    /// `.tgz` waits beside the slot until its tar is encoded into it.
    fn for_slot(slot: &std::path::Path, stores_tar: bool) -> Self {
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
async fn write_under_permit(
    attachment: PendingAttachment,
    kind: AttachmentKind,
    dist: Value,
    paths: AttachmentPaths,
    decodes: &Arc<Semaphore>,
) -> Result<
    Result<Option<(String, Integrity, pnpm_tarball::TarInspection)>, RegistryError>,
    tokio::task::JoinError,
> {
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

/// Write the attachment to `tmp_path`, and for a `.tgz` return its integrity
/// key, the integrity of the tar it decodes to, and its tar metadata.
/// `_decode` is held until the tar is hashed.
fn write_and_derive_tar(
    attachment: &PendingAttachment,
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
    _decode: Option<&OwnedSemaphorePermit>,
) -> Result<Option<(String, Integrity, pnpm_tarball::TarInspection)>, RegistryError> {
    decode_and_write_attachment(attachment, kind, dist, tmp_path)?;
    if kind == AttachmentKind::Sigstore {
        return Ok(None);
    }
    let Some(tgz_integrity) = dist.as_object().and_then(tgz_integrity_key) else { return Ok(None) };
    let tgz = std::fs::File::open(tmp_path)?;
    Ok(tar_integrity_of(std::io::BufReader::new(tgz))
        .map(|(tar, inspection)| (tgz_integrity, tar, inspection)))
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
    super::super::tar_variants::encode_tar(tar, out, None)?
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
