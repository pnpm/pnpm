//! The commit fence on blob incarnations (see
//! [`crate::metadata::BlobRow`]), across instances sharing one database.

use super::{Db, npm, packument, storage};
use crate::{
    BlobId, Storage,
    metadata::{
        ArtifactBlob, BatchDocument, BlobFence, BlobRef, BlobRow, BlobState, PublicationCommit,
        PublishNotes, ReleaseRecords, Uncommitted,
    },
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;

/// The identity of `bytes` in npm's namespace.
pub(super) fn id(bytes: &[u8]) -> BlobId {
    BlobId::of_bytes(Ecosystem::Npm, bytes)
}

/// The `.tgz` of `name@version`, held by incarnation `incarnation` of `id`.
pub(super) fn tgz(name: &str, version: &str, id: &BlobId, incarnation: u64) -> ArtifactBlob {
    ArtifactBlob {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        version: version.to_string(),
        artifact: format!("{name}-{version}.tgz"),
        blob: BlobRef { id: id.clone(), incarnation },
        size: 3,
    }
}

/// Commit `versions` of `name` over what the store holds, mapping
/// `artifacts`.
pub(super) async fn commit(
    storage: &Storage,
    (name, versions): (&str, &[&str]),
    artifacts: Vec<ArtifactBlob>,
) -> Result<PublicationCommit, RegistryError> {
    let current = storage
        .read_hosted_document_for_update(&npm(name))
        .await
        .unwrap();
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(&packument(name, versions)).unwrap(),
        expected: current.map(|current| current.version),
        principal: Some("alice".to_string()),
        records: ReleaseRecords::new(),
    };
    let fence = BlobFence { artifacts, references: Vec::new() };
    storage
        .sql_metadata()
        .commit_fenced_publication(
            "",
            vec![document],
            Vec::new(),
            PublishNotes::default(),
            None,
            fence,
        )
        .await
}

/// The row of `id`, read from the primary.
pub(super) async fn row(storage: &Storage, id: &BlobId) -> Option<BlobRow> {
    storage
        .blob_rows(vec![id.clone()])
        .await
        .unwrap()
        .remove(id)
}

pub(super) fn is_reclaimed(result: &Result<PublicationCommit, RegistryError>) -> bool {
    matches!(result, Err(RegistryError::ArchiveReclaimed { .. }))
}

/// A commit creates the first incarnation of bytes never stored, depends on
/// a live one, and refuses one that is not the live one or the next.
async fn a_commit_creates_the_first_incarnation_and_depends_on_a_live_one(db: Db) {
    let (first, second) = (storage(&db), storage(&db));
    let bytes = id(b"tgz");
    commit(&first, ("pkg", &["1.0.0"]), vec![tgz("pkg", "1.0.0", &bytes, 1)])
        .await
        .unwrap();
    let live = BlobRow { incarnation: 1, state: BlobState::Live, size: 3 };
    assert_eq!(row(&second, &bytes).await, Some(live));
    commit(&second, ("other", &["1.0.0"]), vec![tgz("other", "1.0.0", &bytes, 1)])
        .await
        .unwrap();
    let ahead = commit(&second, ("third", &["1.0.0"]), vec![tgz("third", "1.0.0", &bytes, 2)]);
    assert!(is_reclaimed(&ahead.await));
    let fresh = id(b"never stored");
    let skipped = commit(&second, ("fourth", &["1.0.0"]), vec![tgz("fourth", "1.0.0", &fresh, 2)]);
    assert!(is_reclaimed(&skipped.await));
    assert_eq!(row(&second, &fresh).await, None);
    assert_eq!(row(&second, &bytes).await, Some(live));
}

/// A commit that depends on an incarnation a reclaim took is refused while
/// the reclaim is under way and after it finished; one that stores the
/// bytes again under the next incarnation commits either way.
async fn a_commit_is_refused_while_its_blob_is_reclaiming(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let bytes = id(b"tgz");
    let file = tgz("pkg", "1.0.0", &bytes, 1);
    commit(&publisher, ("pkg", &["0.9.0", "1.0.0"]), vec![file]).await.unwrap();
    // Dropping the version drops its file's mapping.
    commit(&publisher, ("pkg", &["0.9.0"]), Vec::new()).await.unwrap();
    let database = sweeper.sql_metadata();
    let marked = database
        .mark_reclaiming("", Ecosystem::Npm, vec![bytes.clone()])
        .await
        .unwrap();
    assert_eq!(marked, vec![BlobRef { id: bytes.clone(), incarnation: 1 }]);
    let stale = commit(&publisher, ("other", &["1.0.0"]), vec![tgz("other", "1.0.0", &bytes, 1)]);
    assert!(is_reclaimed(&stale.await));
    let again = vec![tgz("other", "1.0.0", &bytes, 2)];
    commit(&publisher, ("other", &["1.0.0"]), again).await.unwrap();
    // The stalled reclaim records only its own incarnation gone.
    database.finish_reclaims("", Ecosystem::Npm, marked).await.unwrap();
    let live = BlobRow { incarnation: 2, state: BlobState::Live, size: 3 };
    assert_eq!(row(&sweeper, &bytes).await, Some(live));
}

/// Once a reclaim finished, the bytes are stored again only under the next
/// incarnation.
async fn a_commit_after_its_blob_was_reclaimed_takes_the_next_incarnation(db: Db) {
    let storage = storage(&db);
    let bytes = id(b"tgz");
    commit(&storage, ("pkg", &["0.9.0", "1.0.0"]), vec![tgz("pkg", "1.0.0", &bytes, 1)])
        .await
        .unwrap();
    commit(&storage, ("pkg", &["0.9.0"]), Vec::new()).await.unwrap();
    let removed = storage
        .reclaim_blobs(vec![bytes.clone()])
        .await
        .unwrap();
    assert_eq!(removed, vec![BlobRef { id: bytes.clone(), incarnation: 1 }]);
    let gone = BlobRow { incarnation: 1, state: BlobState::Gone, size: 3 };
    assert_eq!(row(&storage, &bytes).await, Some(gone));
    let old = commit(&storage, ("other", &["1.0.0"]), vec![tgz("other", "1.0.0", &bytes, 1)]);
    assert!(is_reclaimed(&old.await));
    commit(&storage, ("other", &["1.0.0"]), vec![tgz("other", "1.0.0", &bytes, 2)])
        .await
        .unwrap();
}

/// Two publishers that never saw the bytes both store them as the first
/// incarnation, and both commit.
async fn two_commits_of_one_new_blob_both_land(db: Db) {
    let (first, second) = (storage(&db), storage(&db));
    let bytes = id(b"shared");
    let left = commit(&first, ("left", &["1.0.0"]), vec![tgz("left", "1.0.0", &bytes, 1)]);
    let right = commit(&second, ("right", &["1.0.0"]), vec![tgz("right", "1.0.0", &bytes, 1)]);
    let (left, right) = tokio::join!(left, right);
    left.unwrap();
    right.unwrap();
    let live = BlobRow { incarnation: 1, state: BlobState::Live, size: 3 };
    assert_eq!(row(&first, &bytes).await, Some(live));
}

/// An incarnation a settle fenced off, as `pnpr gc` does with one stored by
/// a publish that may still be on its way, is never committed.
async fn a_fenced_incarnation_is_never_committed(db: Db) {
    let storage = storage(&db);
    let bytes = id(b"in flight");
    let stored =
        vec![Uncommitted { blob: BlobRef { id: bytes.clone(), incarnation: 1 }, may_fence: true }];
    let removable = storage
        .sql_metadata()
        .settle_stored("", Ecosystem::Npm, stored)
        .await
        .unwrap();
    assert_eq!(removable, vec![BlobRef { id: bytes.clone(), incarnation: 1 }]);
    let late = commit(&storage, ("pkg", &["1.0.0"]), vec![tgz("pkg", "1.0.0", &bytes, 1)]);
    assert!(is_reclaimed(&late.await));
    commit(&storage, ("pkg", &["1.0.0"]), vec![tgz("pkg", "1.0.0", &bytes, 2)])
        .await
        .unwrap();
}

conformance! {
    a_commit_creates_the_first_incarnation_and_depends_on_a_live_one,
    a_commit_is_refused_while_its_blob_is_reclaiming,
    a_commit_after_its_blob_was_reclaimed_takes_the_next_incarnation,
    two_commits_of_one_new_blob_both_land,
    a_fenced_incarnation_is_never_committed,
}
