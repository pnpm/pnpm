//! Committing a publication: every document of a request in one transaction
//! of its catalog, with the request's outcome.

use super::{
    Expected, PackageKey,
    adapter::{Connection, Query, Row},
    batch::Batch,
    commit::StoredPackage,
    fence::{FenceCheck, FenceRead},
    notes, publications,
};
use crate::metadata::{
    ArtifactMember, BatchDocument, BlobFence, Generation, PublicationCommit, PublicationRequest,
    PublishNotes, RecordedOutcome,
};
use pnpr_error::{RegistryError, Result};

/// What a publication request commits.
pub(super) struct Submission<'input> {
    pub(super) catalog: &'input str,
    pub(super) documents: &'input [BatchDocument],
    /// The members of other ecosystems than npm, whose versions are
    /// recorded from the documents.
    pub(super) artifacts: &'input [ArtifactMember],
    pub(super) notes: &'input PublishNotes,
    pub(super) request: Option<&'input PublicationRequest>,
    /// The files the commit maps, and the blobs it depends on.
    pub(super) fence: &'input BlobFence,
}

/// Commit every document of `submission` under one new generation of its
/// catalog, or none of them. A request whose identical twin already
/// committed gets that commit's outcome back and commits nothing.
///
/// Everything the commit compares with is read in the round trip that takes
/// the write lock, and everything it writes is sent with the `COMMIT`, so a
/// remote database sees two round trips however many packages, versions,
/// tags, and notes the request carries.
pub(super) fn commit_publication(
    connection: &dyn Connection,
    submission: &Submission<'_>,
) -> Result<PublicationCommit> {
    let reads = Reads::new(submission)?;
    let (mut batch, results) = Batch::begin_with(connection, submission.catalog, &reads.queries())?;
    let read = match reads.answer(&batch, results)? {
        Answer::Replayed(outcome) => return Ok(replayed(submission.catalog, &outcome)),
        Answer::Read(read) => read,
    };
    for ((document, key), stored) in submission.documents
        .iter()
        .zip(&reads.keys)
        .zip(read.stored)
    {
        write_member(&mut batch, document, key, stored)?;
    }
    reads.write_fence(&batch, read.fence.as_ref())?;
    let publication = record(&batch, submission, read.name_taken, &read.noted)?;
    // The batch holds the write lock, so the catalog is at the generation
    // before the batch's until it commits.
    let current = batch.generation() - 1;
    let value = batch.finish()?.unwrap_or(current);
    let generation = Generation { catalog: submission.catalog.to_string(), value };
    Ok(PublicationCommit { generation, publication, replayed: false })
}

/// Record the publication the batch introduced, with the request's outcome
/// and the notes it carries, and return the publication's id.
fn record(
    batch: &Batch<'_>,
    submission: &Submission<'_>,
    name_taken: bool,
    noted: &[bool],
) -> Result<Option<u64>> {
    let Submission {
        documents,
        artifacts,
        notes,
        request,
        ..
    } = *submission;
    let publisher = request
        .map(|request| request.principal.as_str())
        .or_else(|| documents.iter().find_map(|document| document.principal.as_deref()));
    let publication = if batch.changed_anything() {
        publications::record(batch, request, publisher, artifacts, name_taken)?
    } else {
        None
    };
    let members = publications::members_of(batch, artifacts);
    // Notes are checked even when nothing changed, so a request that only
    // brings notes is refused instead of dropping them.
    let introduced = publication.map(|id| (id, members.as_slice()));
    notes::store_publish_notes(batch, notes, introduced, noted, publisher)?;
    Ok(publication)
}

fn replayed(catalog: &str, outcome: &RecordedOutcome) -> PublicationCommit {
    let generation = Generation { catalog: catalog.to_string(), value: outcome.generation };
    PublicationCommit { generation, publication: outcome.publication, replayed: true }
}

/// What a publication reads as its batch begins.
struct Reads<'request> {
    catalog: &'request str,
    request: Option<&'request PublicationRequest>,
    /// The package each document writes.
    keys: Vec<PackageKey>,
    notes: &'request PublishNotes,
    /// The check of the commit's fence, when it maps any file.
    fence: Option<FenceRead>,
}

/// What the reads of a publication found.
enum Answer {
    /// An identical request already committed with this outcome.
    Replayed(RecordedOutcome),
    Read(Read),
}

struct Read {
    /// Whether another publication has the request's publication name.
    name_taken: bool,
    /// What each document's package holds, in document order.
    stored: Vec<StoredPackage>,
    /// Whether each version note's version already has a note.
    noted: Vec<bool>,
    /// What the fence found, when the commit has one.
    fence: Option<FenceCheck>,
}

impl<'request> Reads<'request> {
    /// The reads of a publication of `documents`. A package is written once
    /// per publication, since its reads are made before any write.
    fn new(submission: &Submission<'request>) -> Result<Self> {
        let Submission {
            catalog,
            documents,
            notes,
            request,
            fence,
            ..
        } = *submission;
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
        Ok(Self { catalog, request, keys, notes, fence: FenceRead::new(fence) })
    }

    /// Write what the commit's fence maps and creates, as `check` found it.
    fn write_fence(&self, batch: &Batch<'_>, check: Option<&FenceCheck>) -> Result<()> {
        match (&self.fence, check) {
            (Some(fence), Some(check)) => fence.write(batch.transaction(), self.catalog, check),
            _ => Ok(()),
        }
    }

    fn name(&self) -> Option<&'request str> {
        self.request.and_then(|request| request.name.as_deref())
    }

    fn queries(&self) -> Vec<Query<'_>> {
        let mut queries = Vec::new();
        if let Some(request) = self.request {
            queries.push(publications::claim_query(self.catalog, request));
        }
        if let Some(name) = self.name() {
            queries.push(publications::name_query(self.catalog, name));
        }
        for key in &self.keys {
            queries.extend(StoredPackage::queries(key));
        }
        for note in &self.notes.versions {
            queries.push(notes::version_note_query(self.catalog, note));
        }
        if let Some(fence) = &self.fence {
            queries.extend(fence.queries(self.catalog));
        }
        queries
    }

    /// What the rows of [`Self::queries`] say, read in `batch`.
    fn answer(&self, batch: &Batch<'_>, results: Vec<Vec<Row>>) -> Result<Answer> {
        let mut results = results.into_iter();
        if let Some(request) = self.request
            && let Some(outcome) = publications::claim(batch, request, next(&mut results)?)?
        {
            return Ok(Answer::Replayed(outcome));
        }
        let name_taken = match self.name() {
            Some(_) => !next(&mut results)?.is_empty(),
            None => false,
        };
        let stored = self.keys
            .iter()
            .map(|key| StoredPackage::from_rows(key, &mut results))
            .collect::<Result<_>>()?;
        let noted = self.notes.versions
            .iter()
            .map(|_| Ok(!next(&mut results)?.is_empty()))
            .collect::<Result<_>>()?;
        let fence = self.fence
            .as_ref()
            .map(|fence| fence.check(&mut results))
            .transpose()?;
        Ok(Answer::Read(Read { name_taken, stored, noted, fence }))
    }
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
