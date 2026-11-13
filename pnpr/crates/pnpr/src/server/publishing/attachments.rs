pub(super) use slot_writes::{write_attachment_slot, write_attachment_slots};

use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentVersion, Identity,
    Integrity, Ownership, PendingAttachment, RegistryError, RegistrySource, Storage, Value,
    WriteTarget, authorize_against_packument, authorize_package, authorize_with_ownership,
    cleanup_tmp_slots, extract_attachments, generated_archive, hosted_storage,
    manifest::{
        check_publishable_versions, parse_hosted_packument, prepare_staged_document,
        record_publisher, restore_submitted_tgz_integrities, serialize_staged_document,
    },
    provenance, release_records, resolve_write_target,
    tombstones::{check_publish_tombstone, check_staged_tombstone},
    variants::{self, Declarations, PublishedVariant, VariantTar},
    verification::{describe_generated_archive, verify_published_manifests},
};
use pnpr_storage::{
    BlobId,
    database::{ArtifactBlob, BlobLookup, BlobRow},
    publish::{now_iso, sigstore_bundle_bytes},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Semaphore;

mod preparation;
mod slot_writes;
mod verified_documents;

use preparation::prepare_attachments;
use verified_documents::write_verified;

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
    /// The hosted registry whose `publish` rule admitted [`Self::publisher`].
    pub(in super::super) registry: String,
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
        Some(&target.namespace),
    )
    .await?;

    let storage = hosted_storage(state, Some(&target.namespace));
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
    let registry = target.source.clone();
    let validated =
        ValidatedPublish { name, incoming, prepared, publisher, registry, tarballs, variants };
    Ok((validated, target))
}

/// Accept a new npm name or preflight an existing package's metadata update.
pub(in super::super) async fn validate_staged_publish_name(
    state: &AppState,
    mut doc: ValidatedPublish,
    namespace: &str,
) -> Result<(), RegistryError> {
    let invalid = match doc.name.validate_npm_publish() {
        Ok(()) => return Ok(()),
        Err(error @ RegistryError::InvalidPackageName { .. }) => error,
        Err(error) => return Err(error),
    };
    if !doc.prepared.is_empty() {
        return Err(invalid);
    }
    let storage = hosted_storage(state, Some(namespace));
    let mut base = read_staged_base(state, &storage, Some(namespace), &doc).await?;
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
    /// Hosted storage namespace this publish targets, or `None` for the
    /// default hosted namespace. Threaded into the commit so the write
    /// lands in the right namespace.
    pub(in super::super) namespace: Option<String>,
    /// Who published it.
    pub(in super::super) publisher: Identity,
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
    pub(in super::super) records: pnpr_storage::database::ReleaseRecords,
    /// What the publish introduces, as the `package.publish` audit record
    /// names it.
    pub(in super::super) releases: Vec<String>,
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
    namespace: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let storage = hosted_storage(state, namespace);

    let mut base = read_staged_base(state, &storage, namespace, &doc).await?;
    let tombstone = check_staged_tombstone(state, &storage, &doc, base.packument.is_some()).await?;
    reauthorize_staged(state, &doc, &base, tombstone.as_ref())?;
    if base.version.is_none() {
        base.version =
            tombstone.map(|tombstone| HostedDocumentVersion::Tombstone(tombstone.maintainers));
    }
    let principal = doc.publisher.user_id().map(str::to_string);
    let (merged, without_any) =
        prepare_publish_document(&storage, &mut doc, &mut base, now_iso).await?;
    drop(doc.incoming);
    let ValidatedPublish {
        name,
        prepared,
        tarballs,
        variants,
        publisher,
        ..
    } = doc;

    let verified = write_verified(
        (&storage, principal.as_deref(), &state.inner.proxy.tar_decodes),
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
        namespace: namespace.map(str::to_string),
        publisher,
        verification: verified.verification,
    })
}

/// Repeat the `publish` check against the document or tombstone read under the
/// package lock. The first check ran before the lock, so another publish or
/// unpublish may have changed the package's ownership since.
fn reauthorize_staged(
    state: &AppState,
    doc: &ValidatedPublish,
    base: &StagedBase,
    tombstone: Option<&pnpr_storage::PackageTombstone>,
) -> Result<(), RegistryError> {
    let source = RegistrySource::Hosted(doc.registry.clone());
    if let Some(packument) = &base.packument {
        let package = (doc.name.as_str(), packument);
        return authorize_against_packument(
            state,
            &doc.publisher,
            &source,
            package,
            Action::Publish,
        );
    }
    let ownership = match tombstone {
        Some(tombstone) => Ownership::Owners(&tombstone.maintainers),
        None => Ownership::Unclaimed,
    };
    let package = (doc.name.as_str(), &ownership);
    authorize_with_ownership(state, &doc.publisher, &source, package, Action::Publish)
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
/// expects to store, after reconciling the namespace's transparency log so any
/// rolled-back versions are already marked removed.
async fn read_staged_base(
    state: &AppState,
    storage: &Storage,
    namespace: Option<&str>,
    doc: &ValidatedPublish,
) -> Result<StagedBase, RegistryError> {
    let log = pnpr_storage::transparency_log::LogId {
        namespace: namespace.unwrap_or_default().to_string(),
        ecosystem: Ecosystem::Npm,
    };
    super::super::release_record_logs::ensure_reconciled(state, &log).await?;
    let lookup = BlobLookup { ids: expected_blobs(doc), artifacts: Vec::new() };
    let base = storage.read_hosted_document_for_publish(&doc.name, &lookup).await?;
    staged_base(&doc.name, base)
}

/// The packument a publish of `name` is computed from, as `base` read it.
fn staged_base(
    name: &CanonicalPackageName,
    base: pnpr_storage::database::PublishBase,
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
