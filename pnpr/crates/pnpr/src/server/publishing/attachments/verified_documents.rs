//! Writing a publish's archives and verifying them into the document to
//! record.

use super::{
    Arc,
    CanonicalPackageName,
    PreparedAttachment,
    RegistryError,
    Semaphore,
    Storage,
    TarballStorage,
    Value,
    Verification,
    WrittenAttachments,
    cleanup_tmp_slots,
    provenance,
    release_records,
    serialize_staged_document,
    verify_published_manifests,
    write_attachment_slots,
};

/// Write every attachment of `prepared` and verify the archives against
/// the merged document and the version's provenance statement.
pub(super) async fn write_verified(
    (storage, claimant, decodes): (&Storage, Option<&str>, &Arc<Semaphore>),
    name: &CanonicalPackageName,
    (prepared, tarballs): (Vec<PreparedAttachment>, TarballStorage),
    mut merged: Value,
) -> Result<VerifiedDocument, RegistryError> {
    let statements = provenance::read_statements(&prepared);
    // Stream-decode + verify + write each tarball. A mismatch (or a
    // missing integrity field) short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let written =
        write_attachment_slots((storage, claimant), name, prepared, tarballs, decodes).await?;
    match verify_written(name, &mut merged, &statements, &written) {
        Ok((notices, records)) => Ok(VerifiedDocument {
            document: serialize_staged_document(merged, &written.tars),
            slots: written.slots,
            verification: Verification {
                notices,
                releases: records.keys().cloned().collect(),
                records,
            },
        }),
        Err(err) => {
            cleanup_tmp_slots(written.slots).await;
            Err(err)
        }
    }
}

/// A publish document whose archives agree with its version metadata.
pub(super) struct VerifiedDocument {
    pub(super) document: Vec<u8>,
    pub(super) slots: Vec<pnpr_storage::BlobSlot>,
    pub(super) verification: Verification,
}

/// Verify every archive `written` holds against the version metadata in
/// `merged` and against `statements`, and build the release record of each
/// version.
fn verify_written(
    name: &CanonicalPackageName,
    merged: &mut Value,
    statements: &provenance::Statements,
    written: &WrittenAttachments,
) -> Result<(Vec<String>, pnpr_storage::database::ReleaseRecords), RegistryError> {
    let notices = verify_published_manifests(name, merged, &written.tars)?;
    provenance::check_statements(name, statements, &written.tars)?;
    let records = release_records::accepted_records(name, merged, &written.tars)?;
    Ok((notices, records))
}
