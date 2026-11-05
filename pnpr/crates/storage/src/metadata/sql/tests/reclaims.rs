//! The fence between reclaiming unreferenced archives and the commits that
//! depend on them, across instances sharing one database.

use super::{Db, npm, packument, storage};
use crate::{
    Storage,
    metadata::{
        ArchiveFence, ArchiveRef, BatchDocument, PublicationCommit, PublishNotes, ReclaimTarget,
        ReleaseRecords,
    },
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use serde_json::Value;

const ARTIFACT: &str = "pkg-1.0.0.tgz";
const OWNER: &str = "reclaim-a";

fn archive(name: &str, artifact: &str) -> ArchiveRef {
    ArchiveRef { ecosystem: Ecosystem::Npm, name: name.to_string(), artifact: artifact.to_string() }
}

fn target(name: &str, expected: Option<u64>) -> ReclaimTarget {
    ReclaimTarget {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        expected,
        artifacts: vec![ARTIFACT.to_string()],
    }
}

/// Read `name` as a publish does before it promotes its archives: the
/// package's generation and the fence token.
async fn read_base(storage: &Storage, name: &str) -> (Option<u64>, u64) {
    let base = storage
        .for_hosted("acme", Ecosystem::Npm)
        .read_hosted_document_for_publish(&npm(name))
        .await
        .unwrap();
    (base.package_generation(), base.catalog_generation)
}

/// Commit `document` of `name` over `expected`, depending on its archive
/// read under `token`.
async fn commit(
    storage: &Storage,
    name: &str,
    document: &Value,
    expected: Option<u64>,
    token: u64,
) -> Result<PublicationCommit, RegistryError> {
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(document).unwrap(),
        expected: expected.map(crate::HostedDocumentVersion::Generation),
        principal: Some("alice".to_string()),
        records: ReleaseRecords::new(),
    };
    let fence = ArchiveFence { token, archives: vec![archive(name, ARTIFACT)] };
    storage
        .sql_metadata()
        .commit_fenced_publication(
            "acme",
            vec![document],
            Vec::new(),
            PublishNotes::default(),
            None,
            fence,
        )
        .await
}

/// A reclaim that found the package without the archive's version loses to
/// a commit that lands first and adds it.
async fn a_commit_before_the_mark_keeps_its_archive(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let (expected, _) = read_base(&sweeper, "pkg").await;
    assert_eq!(expected, None);
    let (base, token) = read_base(&publisher, "pkg").await;
    commit(&publisher, "pkg", &packument("pkg", &["1.0.0"]), base, token).await.unwrap();

    let marked = sweeper
        .sql_metadata()
        .mark_reclaims("acme", OWNER, vec![target("pkg", expected)])
        .await
        .unwrap();
    assert!(marked.is_empty(), "{marked:?}");
    assert!(
        sweeper
            .sql_metadata()
            .pending_reclaims("acme")
            .await
            .unwrap()
            .is_empty(),
    );
}

/// A publish that promoted its archive before a reclaim removed it is
/// refused, whether its commit lands while the reclaim is pending or after
/// it finished.
async fn a_commit_after_its_archive_was_reclaimed_is_refused(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let (base, token) = read_base(&publisher, "pkg").await;
    let document = packument("pkg", &["1.0.0"]);

    let (expected, _) = read_base(&sweeper, "pkg").await;
    let database = sweeper.sql_metadata();
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", expected)])
        .await
        .unwrap();
    assert_eq!(marked, vec![archive("pkg", ARTIFACT)]);
    assert_eq!(database.pending_reclaims("acme").await.unwrap(), marked);
    let err = commit(&publisher, "pkg", &document, base, token).await.unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");

    database.finish_reclaims("acme", OWNER, marked).await.unwrap();
    assert!(
        database
            .pending_reclaims("acme")
            .await
            .unwrap()
            .is_empty(),
    );
    let err = commit(&publisher, "pkg", &document, base, token).await.unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");
    assert!(
        publisher
            .for_hosted("acme", Ecosystem::Npm)
            .read_hosted_document(&npm("pkg"))
            .await
            .unwrap()
            .is_none(),
    );
}

/// A publish that read the catalog after the reclaim finished wrote its
/// archive afresh, so it commits.
async fn a_publish_after_the_reclaim_finished_commits(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let database = sweeper.sql_metadata();
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", None)])
        .await
        .unwrap();
    database.finish_reclaims("acme", OWNER, marked).await.unwrap();

    let (base, token) = read_base(&publisher, "pkg").await;
    commit(&publisher, "pkg", &packument("pkg", &["1.0.0"]), base, token).await.unwrap();
    // Another archive of the package, never reclaimed, commits under any
    // token.
    let document = packument("other", &["1.0.0"]);
    commit(&publisher, "other", &document, None, 0).await.unwrap();
}

/// A package without metadata is reclaimed only while it still has none,
/// and one with metadata only at the generation the reclaim read.
async fn a_reclaim_holds_only_while_the_package_is_as_it_was_read(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let database = sweeper.sql_metadata();
    let (base, token) = read_base(&publisher, "pkg").await;
    commit(&publisher, "pkg", &packument("pkg", &["0.9.0"]), base, token).await.unwrap();
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", None)])
        .await
        .unwrap();
    assert!(marked.is_empty(), "{marked:?}");

    let (expected, _) = read_base(&sweeper, "pkg").await;
    assert!(expected.is_some());
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", expected)])
        .await
        .unwrap();
    assert_eq!(marked, vec![archive("pkg", ARTIFACT)]);
}

/// An archive one reclaim marked is not another's to mark, and another
/// reclaim takes it over only once its lease is out: the first reclaim then
/// finds it is no longer its own, so it neither removes nor finishes it.
async fn a_marked_archive_belongs_to_one_reclaim_at_a_time(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", None)])
        .await
        .unwrap();
    assert_eq!(marked, vec![archive("pkg", ARTIFACT)]);
    let other = "reclaim-b";
    let marked = database
        .mark_reclaims("acme", other, vec![target("pkg", None)])
        .await
        .unwrap();
    assert!(marked.is_empty(), "{marked:?}");

    let lease = std::time::Duration::from_hours(1);
    let claimed = database.claim_stale_reclaims("acme", other, lease, |_| true).await.unwrap();
    assert!(claimed.is_empty(), "{claimed:?}");
    let claimed = database
        .claim_stale_reclaims("acme", other, std::time::Duration::ZERO, |_| true)
        .await
        .unwrap();
    assert_eq!(claimed, vec![archive("pkg", ARTIFACT)]);

    let owned = |owner: &'static str| {
        database.owned_reclaims("acme", owner, vec![archive("pkg", ARTIFACT)])
    };
    assert!(owned(OWNER).await.unwrap().is_empty());
    assert_eq!(owned(other).await.unwrap(), vec![archive("pkg", ARTIFACT)]);
    database
        .finish_reclaims("acme", OWNER, vec![archive("pkg", ARTIFACT)])
        .await
        .unwrap();
    assert_eq!(database.pending_reclaims("acme").await.unwrap(), vec![archive("pkg", ARTIFACT)]);
    database
        .finish_reclaims("acme", other, vec![archive("pkg", ARTIFACT)])
        .await
        .unwrap();
    assert!(
        database
            .pending_reclaims("acme")
            .await
            .unwrap()
            .is_empty(),
    );
}

/// Dropping the row of a reclaim finished a day ago keeps its generation as
/// the catalog's horizon: a publish that read an older generation is still
/// refused, however long it took.
async fn a_dropped_reclaim_still_fences_older_publishes(db: Db) {
    let (sweeper, publisher) = (storage(&db), storage(&db));
    let (base, token) = read_base(&publisher, "pkg").await;
    let database = sweeper.sql_metadata();
    let marked = database
        .mark_reclaims("acme", OWNER, vec![target("pkg", None)])
        .await
        .unwrap();
    database.finish_reclaims("acme", OWNER, marked).await.unwrap();
    execute(&db, "UPDATE archive_reclaims SET done_at = done_at - 2 * 24 * 60 * 60").await;
    // Any later reclaim step drops the old row.
    database
        .mark_reclaims("acme", OWNER, vec![target("other", None)])
        .await
        .unwrap();
    assert_eq!(database.pending_reclaims("acme").await.unwrap(), vec![archive("other", ARTIFACT)]);

    let err = commit(&publisher, "pkg", &packument("pkg", &["1.0.0"]), base, token)
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");
    let (base, token) = read_base(&publisher, "pkg").await;
    commit(&publisher, "pkg", &packument("pkg", &["1.0.0"]), base, token).await.unwrap();
}

/// Run `statement` on the database directly, as an operator's client would.
async fn execute(db: &Db, statement: &'static str) {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| {
            let transaction = crate::metadata::sql::batch::WriteTransaction::begin(connection)?;
            transaction.execute(statement, params![])?;
            transaction.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
}

conformance! {
    a_marked_archive_belongs_to_one_reclaim_at_a_time,
    a_dropped_reclaim_still_fences_older_publishes,
    a_commit_before_the_mark_keeps_its_archive,
    a_commit_after_its_archive_was_reclaimed_is_refused,
    a_publish_after_the_reclaim_finished_commits,
    a_reclaim_holds_only_while_the_package_is_as_it_was_read,
}
