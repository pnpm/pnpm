//! Committing a publication: every document of a request in one transaction
//! of its catalog, with the request's outcome.

use super::{
    Expected, PackageKey,
    adapter::{Connection, Query, Row},
    batch::Batch,
    commit::StoredPackage,
    notes, publications,
};
use crate::metadata::{
    ArtifactMember, BatchDocument, Generation, PublicationCommit, PublicationRequest, PublishNotes,
};
use pnpr_error::{RegistryError, Result};

/// Commit every document under one new generation of `catalog`, or none of
/// them. A request whose identical twin already committed gets that commit's
/// outcome back and commits nothing.
///
/// Everything the commit compares with is read in the round trip that takes
/// the write lock, and everything it writes is sent with the `COMMIT`, so a
/// remote database sees two round trips however many packages, versions,
/// tags, and notes the request carries.
pub(super) fn commit_publication(
    connection: &dyn Connection,
    catalog: &str,
    documents: &[BatchDocument],
    artifacts: &[ArtifactMember],
    notes: &PublishNotes,
    request: Option<&PublicationRequest>,
) -> Result<PublicationCommit> {
    let keys = package_keys(catalog, documents)?;
    let name = request.and_then(|request| request.name.as_deref());
    let mut reads: Vec<Query<'_>> = Vec::new();
    if let Some(request) = request {
        reads.push(publications::claim_query(catalog, request));
    }
    if let Some(name) = name {
        reads.push(publications::name_query(catalog, name));
    }
    for key in &keys {
        reads.extend(StoredPackage::queries(key));
    }
    for note in &notes.versions {
        reads.push(notes::version_note_query(catalog, note));
    }
    let (mut batch, results) = Batch::begin_with(connection, catalog, &reads)?;
    let mut results = results.into_iter();
    if let Some(request) = request
        && let Some(outcome) = publications::claim(&batch, request, next(&mut results)?)?
    {
        let generation = Generation { catalog: catalog.to_string(), value: outcome.generation };
        return Ok(PublicationCommit {
            generation,
            publication: outcome.publication,
            replayed: true,
        });
    }
    let name_taken = match name {
        Some(_) => !next(&mut results)?.is_empty(),
        None => false,
    };
    let stored = keys
        .iter()
        .map(|key| StoredPackage::from_rows(key, &mut results))
        .collect::<Result<Vec<_>>>()?;
    let noted = notes.versions
        .iter()
        .map(|_| Ok(!next(&mut results)?.is_empty()))
        .collect::<Result<Vec<_>>>()?;
    for ((document, key), stored) in documents.iter().zip(&keys).zip(stored) {
        write_member(&mut batch, document, key, stored)?;
    }
    let publisher = request
        .map(|request| request.principal.as_str())
        .or_else(|| documents.iter().find_map(|document| document.principal.as_deref()));
    let publication = if batch.changed_anything() {
        publications::record(&batch, request, publisher, artifacts, name_taken)?
    } else {
        None
    };
    let members = publications::members_of(&batch, artifacts);
    // Notes are checked even when nothing changed, so a request that only
    // brings notes is refused instead of dropping them.
    let introduced = publication.map(|id| (id, members.as_slice()));
    notes::store_publish_notes(&batch, notes, introduced, &noted, publisher)?;
    // The batch holds the write lock, so the catalog is at the generation
    // before the batch's until it commits.
    let current = batch.generation() - 1;
    let value = batch.finish()?.unwrap_or(current);
    let generation = Generation { catalog: catalog.to_string(), value };
    Ok(PublicationCommit { generation, publication, replayed: false })
}

/// The package each document writes. A package is written once per
/// publication, since its reads are made before any write.
fn package_keys(catalog: &str, documents: &[BatchDocument]) -> Result<Vec<PackageKey>> {
    let mut keys: Vec<PackageKey> = Vec::with_capacity(documents.len());
    for document in documents {
        let key = PackageKey {
            catalog: catalog.to_string(),
            ecosystem: document.ecosystem,
            name: document.name.clone(),
        };
        if keys.contains(&key) {
            return Err(RegistryError::BadRequest {
                reason: format!("a publication names package {} more than once", document.name),
            });
        }
        keys.push(key);
    }
    Ok(keys)
}

/// The rows of the next read of the batch.
fn next(results: &mut impl Iterator<Item = Vec<Row>>) -> Result<Vec<Row>> {
    results
        .next()
        .ok_or_else(|| RegistryError::Internal {
            reason: "a metadata batch did not answer every read of a publication".to_string(),
        })
}

fn write_member(
    batch: &mut Batch<'_>,
    document: &BatchDocument,
    key: &PackageKey,
    stored: StoredPackage,
) -> Result<()> {
    let expected = Expected::from_version(document.expected.as_ref())?;
    let principal = document.principal.as_deref();
    let records = &document.records;
    let commit = batch.package(key);
    match commit.write_document(stored, &document.bytes, expected, principal, records)? {
        None => Err(RegistryError::DocumentWriteConflict { package: document.name.clone() }),
        Some(true) => {
            batch.touch(key);
            Ok(())
        }
        Some(false) => Ok(()),
    }
}
