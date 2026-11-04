//! Writing a publish's archives and verifying them, with the archive pnpr
//! generates for a version without an `any` archive, into the document to
//! record.

use super::{
    Arc, BTreeSet, CanonicalPackageName, Declarations, PreparedAttachment, RegistryError,
    Semaphore, Storage, TarballStorage, Value, Verification, WrittenAttachments, cleanup_tmp_slots,
    describe_generated_archive, generated_archive, provenance, release_records,
    serialize_staged_document, variants, verify_published_manifests, write_attachment_slot,
    write_attachment_slots,
};

/// Write every attachment of `prepared` and verify the archives against
/// the merged document and the version's provenance statement.
pub(super) async fn write_verified(
    (storage, claimant, decodes): (&Storage, Option<&str>, &Arc<Semaphore>),
    name: &CanonicalPackageName,
    (prepared, tarballs): (Vec<PreparedAttachment>, TarballStorage),
    (merged, declared, without_any): (Value, &Declarations, &BTreeSet<String>),
) -> Result<VerifiedDocument, RegistryError> {
    let statements = provenance::read_statements(&prepared);
    // Stream-decode + verify + write each tarball. A mismatch — or a
    // missing integrity field — short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let written =
        write_attachment_slots((storage, claimant), name, prepared, tarballs, decodes).await?;
    let writer = SlotWriter { storage, tarballs, decodes };
    let checks = PublishChecks { declared, statements: &statements, without_any };
    verified_document(name, (merged, checks), written, writer).await
}

/// A publish document whose archives agree with its version metadata.
pub(super) struct VerifiedDocument {
    pub(super) document: Vec<u8>,
    pub(super) slots: Vec<pnpr_storage::BlobSlot>,
    pub(super) verification: Verification,
}

/// What a publish's archives are checked against beside its version
/// metadata.
#[derive(Clone, Copy)]
struct PublishChecks<'a> {
    /// The variants each version references.
    declared: &'a Declarations,
    /// Each version's provenance statement.
    statements: &'a provenance::Statements,
    /// The versions published without an `any` archive.
    without_any: &'a BTreeSet<String>,
}

/// Where a publish writes the archives pnpr adds to it.
#[derive(Clone, Copy)]
struct SlotWriter<'a> {
    storage: &'a Storage,
    tarballs: TarballStorage,
    decodes: &'a Arc<Semaphore>,
}

/// Verify every archive `written` holds against the version metadata in
/// `merged` and against `checks`, record the verified variants on it,
/// generate the archive of each version without an `any` archive, and serialize the
/// verified document. A refused archive removes the written slots.
async fn verified_document(
    name: &CanonicalPackageName,
    (mut merged, checks): (Value, PublishChecks<'_>),
    mut written: WrittenAttachments,
    writer: SlotWriter<'_>,
) -> Result<VerifiedDocument, RegistryError> {
    match verify_written(name, &mut merged, checks, &mut written, writer).await {
        Ok((notices, records)) => Ok(VerifiedDocument {
            document: serialize_staged_document(merged, &written.tars),
            slots: written.slots,
            verification: Verification { notices, records },
        }),
        Err(err) => {
            cleanup_tmp_slots(written.slots).await;
            Err(err)
        }
    }
}

async fn verify_written(
    name: &CanonicalPackageName,
    merged: &mut Value,
    checks: PublishChecks<'_>,
    written: &mut WrittenAttachments,
    writer: SlotWriter<'_>,
) -> Result<(Vec<String>, pnpr_storage::metadata::ReleaseRecords), RegistryError> {
    let notices = verify_published_manifests(name, merged, &written.tars)?;
    let archives = (written.variants.as_slice(), written.tars.as_slice());
    variants::attach_variants((name, merged), checks.declared, archives, checks.without_any)?;
    provenance::check_statements(name, checks.statements, &written.tars, &written.variants)?;
    for version in checks.without_any {
        write_generated_archive(name, merged, version, written, writer).await?;
    }
    let records =
        release_records::accepted_records(name, merged, &written.tars, &written.variants)?;
    Ok((notices, records))
}

/// Write the archive pnpr generates for `version`, published without an
/// `any` archive,
/// and describe the version by it in npm's documents.
async fn write_generated_archive(
    name: &CanonicalPackageName,
    merged: &mut Value,
    version: &str,
    written: &mut WrittenAttachments,
    writer: SlotWriter<'_>,
) -> Result<(), RegistryError> {
    let manifest = merged
        .get_mut("versions")
        .and_then(|versions| versions.get_mut(version))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!("{}@{version} has no version metadata", name.as_str()),
        })?;
    let prepared = generated_archive::attachment(name, version, &Value::Object(manifest.clone()));
    let dist = prepared.dist.clone();
    let filename = prepared.canonical.clone();
    let (slot, tar) = write_attachment_slot(
        (writer.storage, None),
        name,
        prepared,
        writer.tarballs,
        writer.decodes,
    )
    .await?;
    written.slots.push(slot);
    let Some(mut tar) = tar else {
        return Err(RegistryError::Internal {
            reason: format!("the generated archive of {}@{version} has no tar", name.as_str()),
        });
    };
    tar.generated = true;
    describe_generated_archive(
        manifest,
        &tar.archive,
        dist,
        &format!("{}/-/{filename}", name.as_str()),
    );
    written.tars.push(tar);
    Ok(())
}
