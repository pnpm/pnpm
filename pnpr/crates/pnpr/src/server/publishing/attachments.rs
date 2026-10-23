use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentVersion,
    HostedOriginalRef, Identity, Integrity, JournaledRevisionRef, PendingAttachment, RegistryError,
    RegistrySource, Storage, Value, WriteTarget, authorize_package, cleanup_tmp_slots,
    create_hex_hash, extract_attachments, hosted_storage, integrity_addressed_tarball_path,
    manifest::{
        check_publishable_versions, parse_hosted_packument, prepare_staged_document,
        record_publisher, restore_submitted_tgz_integrities, serialize_staged_document,
    },
    resolve_write_target, stream_decode_verify_and_write, tar_integrity_of, tgz_integrity_key,
    tombstones::{check_publish_tombstone, check_staged_tombstone},
    write_sigstore_bundle,
};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// What kind of payload an attachment carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum AttachmentKind {
    Tarball,
    Sigstore,
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

    // Validate attachment filenames against the package and parse their
    // canonical storage filename, matching version, and payload kind.
    let prepared = prepare_attachments(&name, &incoming, attachments)?;
    let publisher = identity.clone();
    Ok((ValidatedPublish { name, incoming, prepared, publisher }, target))
}

fn prepare_attachments(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachments: Vec<PendingAttachment>,
) -> Result<Vec<PreparedAttachment>, RegistryError> {
    let mut prepared = Vec::with_capacity(attachments.len());
    let mut seen_canonical = HashSet::with_capacity(attachments.len());
    let mut tarball_versions = HashSet::new();

    for attachment in attachments {
        let (canonical, version, kind) = parse_attachment_kind(name, &attachment.filename)?;
        if !seen_canonical.insert(canonical.clone()) {
            return Err(RegistryError::BadRequest {
                reason: format!("duplicate attachment for {canonical}"),
            });
        }
        if kind == AttachmentKind::Tarball {
            tarball_versions.insert(version.clone());
        }
        let dist = incoming
            .get("versions")
            .and_then(|versions| versions.get(&version))
            .and_then(|manifest| manifest.get("dist"))
            .cloned()
            .unwrap_or(Value::Null);
        prepared.push(PreparedAttachment { attachment, canonical, version, dist, kind });
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
    pub(in super::super) revision_refs: Vec<JournaledRevisionRef>,
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
    let ValidatedPublish { name, mut incoming, prepared, .. } = doc;
    restore_submitted_tgz_integrities(&storage, &name, hosted.as_ref(), &mut incoming).await?;

    check_publishable_versions(&name, &incoming, hosted.as_ref(), &prepared)?;

    let merged = prepare_staged_document(&name, hosted.as_ref(), &incoming, &prepared, now_iso);
    // `incoming` is no longer needed; drop it so the base64 strings
    // inside go away as soon as `prepared` (which owns each one) is
    // drained below.
    drop(incoming);

    // Stream-decode + verify + write each tarball. A mismatch — or a
    // missing integrity field — short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let written =
        write_attachment_slots(&storage, &name, prepared, &state.inner.proxy.tar_decodes).await?;
    let document = serialize_staged_document(merged, &written.tars);
    let revision_refs = written.tars
        .iter()
        .filter_map(|tar| staged_hosted_original_ref(&name, tar))
        .collect();
    Ok(StagedPublish {
        name,
        ecosystem: Ecosystem::Npm,
        document,
        base_version,
        slots: written.slots,
        revision_refs,
        org: org.map(str::to_string),
    })
}

/// The blob slots a publish wrote, and the tar each of its `.tgz` files
/// decodes to.
pub(super) struct WrittenAttachments {
    pub(super) slots: Vec<pnpr_storage::BlobSlot>,
    pub(super) tars: Vec<PublishedTar>,
}

/// A published `.tgz` whose whole gzip stream decoded to a tar.
pub(super) struct PublishedTar {
    pub(super) version: String,
    pub(super) filename: String,
    pub(super) integrity: Integrity,
    pub(super) inspection: pnpm_tarball::TarInspection,
}

/// Write every attachment into its reserved blob slot. A failure removes
/// the slots written so far, so a bad upload leaves no on-disk artifact.
pub(super) async fn write_attachment_slots(
    storage: &Storage,
    name: &CanonicalPackageName,
    prepared: Vec<PreparedAttachment>,
    decodes: &Arc<Semaphore>,
) -> Result<WrittenAttachments, RegistryError> {
    let mut written =
        WrittenAttachments { slots: Vec::with_capacity(prepared.len()), tars: Vec::new() };
    for attachment in prepared {
        match write_attachment_slot(storage, name, attachment, decodes).await {
            Ok((slot, tar)) => {
                written.slots.push(slot);
                written.tars.extend(tar);
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
/// and is published without one. A tarball holds a permit from `decodes`
/// until its tar is hashed.
pub(super) async fn write_attachment_slot(
    storage: &Storage,
    name: &CanonicalPackageName,
    prepared: PreparedAttachment,
    decodes: &Arc<Semaphore>,
) -> Result<(pnpr_storage::BlobSlot, Option<PublishedTar>), RegistryError> {
    let slot = storage.reserve_hosted_blob(name, &prepared.canonical).await?;
    let result = write_under_permit(
        prepared.attachment,
        prepared.kind,
        prepared.dist,
        slot.tmp_path.clone(),
        decodes,
    )
    .await;
    match result {
        Ok(Ok(None)) => Ok((slot, None)),
        Ok(Ok(Some((tgz_integrity, integrity, inspection)))) => {
            // A tar registry's first tar response records a binding publish
            // failed to.
            let derived = pnpr_storage::DerivedTar::new(
                integrity.to_string(),
                inspection.requires_build,
                inspection.file_count,
                inspection.unpacked_size,
            )
            .with_bins(inspection.bins.clone());
            if let Err(err) = storage.record_tar_integrity(name, &tgz_integrity, derived).await {
                tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
            }
            Ok((
                slot,
                Some(PublishedTar {
                    version: prepared.version,
                    filename: prepared.canonical,
                    integrity,
                    inspection,
                }),
            ))
        }
        Ok(Err(err)) => Err(err),
        Err(join_err) => {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            Err(RegistryError::Io(std::io::Error::other(join_err.to_string())))
        }
    }
}

/// Run [`write_and_derive_tar`] on a blocking thread. A tarball waits for a
/// permit from `decodes` first.
async fn write_under_permit(
    attachment: PendingAttachment,
    kind: AttachmentKind,
    dist: Value,
    tmp_path: std::path::PathBuf,
    decodes: &Arc<Semaphore>,
) -> Result<
    Result<Option<(String, Integrity, pnpm_tarball::TarInspection)>, RegistryError>,
    tokio::task::JoinError,
> {
    let permit = match kind {
        AttachmentKind::Tarball => Some(
            Arc::clone(decodes)
                .acquire_owned()
                .await
                .expect("the decode semaphore is never closed"),
        ),
        AttachmentKind::Sigstore => None,
    };
    tokio::task::spawn_blocking(move || {
        write_and_derive_tar(&attachment, kind, &dist, &tmp_path, permit)
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
    _decode: Option<OwnedSemaphorePermit>,
) -> Result<Option<(String, Integrity, pnpm_tarball::TarInspection)>, RegistryError> {
    decode_and_write_attachment(attachment, kind, dist, tmp_path)?;
    if kind != AttachmentKind::Tarball {
        return Ok(None);
    }
    let Some(tgz_integrity) = dist.as_object().and_then(tgz_integrity_key) else { return Ok(None) };
    let tgz = std::fs::File::open(tmp_path)?;
    Ok(tar_integrity_of(std::io::BufReader::new(tgz))
        .map(|(tar, inspection)| (tgz_integrity, tar, inspection)))
}

fn decode_and_write_attachment(
    attachment: &PendingAttachment,
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
) -> Result<(), RegistryError> {
    let dist_ref = (!dist.is_null()).then_some(dist);
    match kind {
        AttachmentKind::Tarball => {
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

/// The reference that serves a published version's tar by its digest.
pub(super) fn staged_hosted_original_ref(
    package: &CanonicalPackageName,
    tar: &PublishedTar,
) -> Option<JournaledRevisionRef> {
    let path = integrity_addressed_tarball_path(&tar.integrity)?;
    let digest = path.strip_prefix("-/tarballs/sha512/")?.to_string();
    let record =
        HostedOriginalRef { package: package.as_str().to_string(), version: tar.version.clone() };
    let bytes = serde_json::to_vec(&record).expect("hosted original reference serializes");
    let ref_id = create_hex_hash(&format!("{}\0{}", record.package, record.version));
    Some(JournaledRevisionRef { filename: tar.filename.clone(), digest, ref_id, bytes })
}
