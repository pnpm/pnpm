use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentVersion, Identity,
    Integrity, PendingAttachment, RegistryError, RegistrySource, Storage, Value, WriteTarget,
    authorize_package, cleanup_tmp_slots, extract_attachments, generated_archive, hosted_storage,
    manifest::{
        check_publishable_versions, parse_hosted_packument, prepare_staged_document,
        record_publisher, restore_submitted_tgz_integrities, serialize_staged_document,
    },
    provenance, release_records, resolve_write_target,
    tombstones::{check_publish_tombstone, check_staged_tombstone},
    variants::{self, Declarations, PublishedVariant, VariantTar},
    verification::{describe_generated_archive, verify_published_manifests},
};
use crate::server::staged_archives::claim_staged_archive;
use pnpr_storage::{
    BlobId,
    metadata::{ArtifactBlob, BlobLookup, BlobRow},
    publish::{now_iso, sigstore_bundle_bytes},
};
use slot_writes::{AttachmentPaths, write_under_permit};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod preparation;
mod slot_writes;
mod verified_documents;

use verified_documents::write_verified;

use preparation::prepare_attachments;

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
    /// Attached platform variant archives, by version.
    pub(in super::super) variants: Declarations,
}

/// One publish attachment resolved to its canonical on-disk filename and its
/// `versions[version].dist` block.
pub(in super::super) struct PreparedAttachment {
    pub(in super::super) attachment: PendingAttachment,
    /// Canonical on-disk filename.
    pub(super) canonical: String,
    /// The version this attachment publishes, parsed from its filename.
    /// Lets the re-publish guard tell a content publish from a metadata-only
    /// update (which carries no attachments).
    pub(in super::super) version: String,
    /// The matching `dist` block, or `Value::Null` when absent.
    pub(in super::super) dist: Value,
    pub(in super::super) kind: AttachmentKind,
    /// The tag that names an [`AttachmentKind::Variant`].
    pub(in super::super) variant_tag: Option<String>,
}

impl ValidatedPublish {
    /// The integrities of the staged archives this publish's attachments
    /// reference.
    pub(in super::super) fn staged_integrities(&self) -> impl Iterator<Item = &str> {
        self.prepared.iter().filter_map(|prepared| prepared.attachment.staged_integrity())
    }
}

pub(in super::super) async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: CanonicalPackageName,
    mut incoming: Value,
) -> Result<(ValidatedPublish, WriteTarget), RegistryError> {
    super::check_body_versions(&incoming)?;

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
    variants::remove_server_fields(&mut incoming);
    record_publisher(&mut incoming, identity);

    // Validate attachment filenames against the package and parse their
    // canonical storage filename, matching version, and payload kind.
    let prepared = prepare_attachments(&name, &incoming, attachments)?;
    let variants = variants::attached_variants(&prepared);
    let publisher = identity.clone();
    let tarballs = if super::super::tar_registry::is_tar_registry(state, &target.source) {
        TarballStorage::Tar
    } else {
        TarballStorage::Tgz
    };
    Ok((ValidatedPublish { name, incoming, prepared, publisher, tarballs, variants }, target))
}

/// Accept a new npm name or preflight an existing package's metadata update.
pub(in super::super) async fn validate_staged_publish_name(
    state: &AppState,
    mut doc: ValidatedPublish,
    org: &str,
) -> Result<(), RegistryError> {
    let invalid = match doc.name.validate_npm_publish() {
        Ok(()) => return Ok(()),
        Err(error @ RegistryError::InvalidPackageName { .. }) => error,
        Err(error) => return Err(error),
    };
    if !doc.prepared.is_empty() {
        return Err(invalid);
    }
    let storage = hosted_storage(state, Some(org));
    let mut base = read_staged_base(&storage, &doc).await?;
    prepare_publish_document(&storage, &mut doc, &mut base, &now_iso()).await?;
    Ok(())
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
    /// What the store held when `document` was computed from it.
    pub(in super::super) base: StagedBase,
    pub(in super::super) slots: Vec<pnpr_storage::BlobSlot>,
    /// Hosted-org storage namespace this publish targets, or `None` for the
    /// flat (path-less) hosted store. Threaded into the commit so the write
    /// lands in the right org.
    pub(in super::super) org: Option<String>,
    /// Who published it, recorded with every dist-tag the document moves.
    pub(in super::super) publisher: Option<String>,
    /// What verifying the published archives produced.
    pub(in super::super) verification: Verification,
}

/// The stored document a staged publish was computed from.
#[derive(Default)]
pub(in super::super) struct StagedBase {
    /// A client revision precondition forbids rebasing onto another document.
    pub(in super::super) revision_checked: bool,
    /// Its version, which the commit expects to replace; `None` when there
    /// was none.
    pub(in super::super) version: Option<HostedDocumentVersion>,
    /// The npm packument itself, which a commit that lost the race to
    /// another replica rebases the publish's changes off; `None` for a
    /// first publish and for other ecosystems.
    pub(in super::super) packument: Option<Value>,
    /// The rows of the blobs the publish expected to store, read with the
    /// document, `None` for bytes never stored.
    pub(in super::super) blobs: BTreeMap<BlobId, Option<BlobRow>>,
    /// Files of the package already stored that the document references and
    /// the publish did not stage, such as the layers an image manifest names.
    /// The commit maps them and depends on their blobs as on the ones it
    /// promotes.
    pub(in super::super) references: Vec<ArtifactBlob>,
}

/// What verifying a publish's archives against its version metadata
/// produced, beside the document to record.
#[derive(Default)]
pub(in super::super) struct Verification {
    /// What the publisher should hear about a publish that was accepted with
    /// a difference, sent back as `npm-notice` response headers.
    pub(in super::super) notices: Vec<String>,
    /// The release record of each version the document introduces, which
    /// the metadata store accepts with it.
    pub(in super::super) records: pnpr_storage::metadata::ReleaseRecords,
}

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`](super::commit_publishes). On error, every tmp file this call wrote is
/// removed.
pub(in super::super) async fn stage_publish(
    state: &AppState,
    mut doc: ValidatedPublish,
    now_iso: &str,
    org: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let storage = hosted_storage(state, org);

    let mut base = read_staged_base(&storage, &doc).await?;
    check_staged_tombstone(state, &storage, &doc, base.packument.is_some()).await?;
    let publisher = doc.publisher.user_id().map(str::to_string);
    let (merged, without_any) =
        prepare_publish_document(&storage, &mut doc, &mut base, now_iso).await?;
    drop(doc.incoming);
    let ValidatedPublish {
        name, prepared, tarballs, variants, ..
    } = doc;

    let verified = write_verified(
        (&storage, publisher.as_deref(), &state.inner.proxy.tar_decodes),
        &name,
        (prepared, tarballs),
        (merged, &variants, &without_any),
    )
    .await?;
    Ok(StagedPublish {
        name,
        ecosystem: Ecosystem::Npm,
        document: verified.document,
        base,
        slots: verified.slots,
        org: org.map(str::to_string),
        publisher,
        verification: verified.verification,
    })
}

async fn prepare_publish_document(
    storage: &Storage,
    doc: &mut ValidatedPublish,
    base: &mut StagedBase,
    now_iso: &str,
) -> Result<(Value, BTreeSet<String>), RegistryError> {
    let hosted = base.packument.as_ref();
    restore_submitted_tgz_integrities(storage, &doc.name, hosted, &mut doc.incoming).await?;
    let without_any = check_publishable_versions(
        &doc.name,
        &doc.incoming,
        hosted,
        (&doc.prepared, &doc.variants),
    )?;
    if doc.prepared.is_empty()
        && let Some(hosted) = hosted
    {
        super::super::package_mutation::revisions::prepare_deprecation(
            &doc.name,
            hosted,
            &mut doc.incoming,
        )?;
        base.revision_checked = true;
    } else {
        doc.name.validate_npm_publish()?;
    }
    if let Some(object) = doc.incoming.as_object_mut() {
        object.remove("_rev");
    }
    let merged = prepare_staged_document(&doc.name, hosted, &doc.incoming, &doc.prepared, now_iso);
    Ok((merged, without_any))
}

/// The packument `doc` is computed from, read with the rows of the blobs it
/// expects to store.
async fn read_staged_base(
    storage: &Storage,
    doc: &ValidatedPublish,
) -> Result<StagedBase, RegistryError> {
    let lookup = BlobLookup { ids: expected_blobs(doc), artifacts: Vec::new() };
    let base = storage.read_hosted_document_for_publish(&doc.name, &lookup).await?;
    staged_base(&doc.name, base)
}

/// The packument a publish of `name` is computed from, as `base` read it.
fn staged_base(
    name: &CanonicalPackageName,
    base: pnpr_storage::metadata::PublishBase,
) -> Result<StagedBase, RegistryError> {
    let (packument, version) = parse_hosted_packument(name, base.stored)?;
    Ok(StagedBase { version, packument, blobs: base.blobs, ..StagedBase::default() })
}

/// The identities the bytes `doc` stores will have, where they are known
/// before they are written, so their rows are read with the document: a
/// `.tgz` stored as published has the `sha512` its `dist.integrity`
/// declares, and a sigstore bundle is its own bytes. A tar a tar registry
/// stores in place of the `.tgz`, and the archive pnpr generates for a
/// version without one, are known only once written, and their rows take
/// one more round trip.
fn expected_blobs(doc: &ValidatedPublish) -> Vec<BlobId> {
    let ecosystem = pnpr_registry::Ecosystem::Npm;
    let mut ids: Vec<BlobId> = doc.prepared
        .iter()
        .filter_map(|prepared| match prepared.kind {
            AttachmentKind::Tarball if doc.tarballs == TarballStorage::Tar => None,
            AttachmentKind::Tarball | AttachmentKind::Variant => {
                let integrity = prepared.dist.get("integrity")?.as_str()?;
                BlobId::from_integrity(&integrity.parse().ok()?, ecosystem)
            }
            AttachmentKind::Sigstore => {
                let bytes = sigstore_bundle_bytes(prepared.attachment.inline_data()?).ok()?;
                Some(BlobId::of_bytes(ecosystem, &bytes))
            }
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
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
    /// The `.tgz` integrity as published, which the tar's facts are recorded
    /// under.
    pub(super) tgz_integrity: Option<String>,
    /// The integrity of the tar the `.tgz` decodes to.
    pub(super) integrity: Integrity,
    pub(super) inspection: pnpm_tarball::TarInspection,
    /// What the tar holds that the version's manifest is derived from.
    pub(super) archive: pnpm_tarball::PublicationArchive,
    /// Set for a platform variant's `.tgz`.
    pub(super) variant: Option<VariantTar>,
    /// Whether pnpr generated the archive for a version published without a
    /// `any` archive (see [`generated_archive`]). It is not one of the version's
    /// archives.
    pub(super) generated: bool,
}

/// What a published `.tgz` decodes to: the key its tar integrity is recorded
/// under, the tar's integrity and metadata, and its manifest inputs.
pub(super) struct TarDerivation {
    tgz_integrity: Option<String>,
    integrity: Integrity,
    inspection: pnpm_tarball::TarInspection,
    archive: pnpm_tarball::PublicationArchive,
}

/// Write every attachment into its reserved blob slot. A failure removes
/// the slots written so far, so a bad upload leaves no on-disk artifact.
///
/// `storage` comes with the principal publishing, whose staged archives an
/// attachment may reference.
pub(super) async fn write_attachment_slots(
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
pub(super) async fn write_attachment_slot(
    (storage, claimant): (&Storage, Option<&str>),
    name: &CanonicalPackageName,
    prepared: PreparedAttachment,
    tarballs: TarballStorage,
    decodes: &Arc<Semaphore>,
) -> Result<(pnpr_storage::BlobSlot, Option<PublishedTar>), RegistryError> {
    // A staged archive is read before a slot is reserved: one that is missing
    // or expired fails the publish with nothing to clean up.
    let (staged, permit) = staged_bytes((storage, claimant), &prepared.attachment, decodes).await?;
    let stores_tar = tarballs == TarballStorage::Tar && prepared.kind == AttachmentKind::Tarball;
    let blob = if stores_tar {
        name.tar_name_for_version(&prepared.version)
    } else {
        prepared.canonical.clone()
    };
    let slot = storage.reserve_hosted_blob(name, &blob).await?.for_version(&prepared.version);
    let paths = AttachmentPaths::for_slot(&slot.tmp_path, stores_tar);
    let result = write_under_permit(
        (prepared.attachment, staged),
        prepared.kind,
        prepared.dist,
        paths,
        (decodes, permit),
    )
    .await;
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
    let Some(derived) = derived else { return Ok((slot, None)) };
    if let Some(tgz_integrity) = &derived.tgz_integrity {
        let recorded = record_published_tar(storage, name, tgz_integrity, &derived, stores_tar);
        if let Err(err) = recorded.await {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            return Err(err);
        }
    }
    Ok((slot, Some(PublishedTar::of(prepared.version, prepared.variant_tag, derived))))
}

/// The bytes of the staged archive `attachment` references, read from what
/// `claimant` staged, with the decode permit they were read under; `None`
/// for an attachment carried inline. The permit is the one its tarball
/// needs anyway, so publishes waiting for one do not each hold an archive in
/// memory.
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

impl PublishedTar {
    /// The tar `derived` describes, a platform variant's when a variant tag
    /// names the attachment.
    fn of(version: String, variant_tag: Option<String>, derived: TarDerivation) -> Self {
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
}

/// Record the tar a published `.tgz` decodes to. A failure only warns: a tar
/// registry's first tar response records it later. A stored tar is served
/// without its `.tgz`, so with `stores_tar` nothing would, and the failure
/// fails the publish.
async fn record_published_tar(
    storage: &Storage,
    name: &CanonicalPackageName,
    tgz_integrity: &str,
    derived: &TarDerivation,
    stores_tar: bool,
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
    if stores_tar { Err(err) } else { Ok(()) }
}
