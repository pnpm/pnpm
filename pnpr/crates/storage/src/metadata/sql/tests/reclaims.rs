//! The transitions that reclaim a blob (see [`crate::metadata::BlobRow`]).

use super::{
    Db,
    blobs::{commit, id, row, tgz},
    npm, storage,
};
use crate::metadata::{BlobRef, BlobRow, BlobState, Uncommitted};
use pnpr_package_name::Ecosystem;
use std::time::Duration;

/// A reclaim marks only live blobs nothing maps: dropping a version, or the
/// whole package, drops the mappings that kept its blob.
async fn only_live_blobs_nothing_maps_are_marked(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let (kept, dropped) = (id(b"kept"), id(b"dropped"));
    let files = vec![tgz("pkg", "1.0.0", &kept, 1), tgz("pkg", "2.0.0", &dropped, 1)];
    commit(&storage, ("pkg", &["1.0.0", "2.0.0"]), files).await.unwrap();
    let both = vec![kept.clone(), dropped.clone()];
    assert!(
        database
            .mark_reclaiming("", Ecosystem::Npm, both.clone())
            .await
            .unwrap()
            .is_empty(),
    );
    assert!(
        storage
            .unmapped_blobs()
            .await
            .unwrap()
            .is_empty(),
    );

    commit(&storage, ("pkg", &["1.0.0"]), Vec::new()).await.unwrap();
    let unmapped: Vec<_> = storage
        .unmapped_blobs()
        .await
        .unwrap()
        .into_keys()
        .collect();
    assert_eq!(unmapped, vec![dropped.clone()]);
    let marked = database.mark_reclaiming("", Ecosystem::Npm, both).await.unwrap();
    assert_eq!(marked, vec![BlobRef { id: dropped, incarnation: 1 }]);

    assert!(
        storage
            .remove_package(&npm("pkg"))
            .await
            .unwrap(),
    );
    let gone = BlobRow { incarnation: 1, state: BlobState::Gone, size: 3 };
    assert_eq!(row(&storage, &kept).await, Some(gone));
}

/// What a settle removes, keeps, and fences off of incarnations no commit
/// made the live one.
async fn a_settle_removes_only_what_no_commit_can_make_live(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let live = id(b"live");
    commit(&storage, ("pkg", &["1.0.0"]), vec![tgz("pkg", "1.0.0", &live, 1)])
        .await
        .unwrap();
    let unseen = id(b"unseen");
    let at = |id: &crate::BlobId, incarnation| BlobRef { id: id.clone(), incarnation };
    let candidates = vec![fenced(at(&live, 1)), fenced(at(&live, 2)), unfenced(at(&unseen, 3))];
    let settle = |stored| database.settle_stored("", Ecosystem::Npm, stored);
    assert!(settle(candidates).await.unwrap().is_empty());
    assert_eq!(row(&storage, &unseen).await, None);

    let taken = settle(vec![fenced(at(&unseen, 3))]).await.unwrap();
    assert_eq!(taken, vec![at(&unseen, 3)]);
    let gone = BlobRow { incarnation: 3, state: BlobState::Gone, size: 0 };
    assert_eq!(row(&storage, &unseen).await, Some(gone));
    // Below the current incarnation, or at it once gone, nothing can commit.
    let dead = vec![unfenced(at(&unseen, 2)), unfenced(at(&unseen, 3))];
    assert_eq!(settle(dead).await.unwrap(), vec![at(&unseen, 2), at(&unseen, 3)]);
}

/// A reclaim records only its own incarnation gone, and another run takes
/// over one left `reclaiming` only once the lease is out.
async fn a_stopped_reclaim_is_taken_over_after_its_lease(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let bytes = id(b"tgz");
    commit(&storage, ("pkg", &["0.9.0", "1.0.0"]), vec![tgz("pkg", "1.0.0", &bytes, 1)])
        .await
        .unwrap();
    commit(&storage, ("pkg", &["0.9.0"]), Vec::new()).await.unwrap();
    let marked = database
        .mark_reclaiming("", Ecosystem::Npm, vec![bytes.clone()])
        .await
        .unwrap();
    let reclaiming = BlobRow { incarnation: 1, state: BlobState::Reclaiming, size: 3 };
    let kept = vec![fenced(marked[0].clone())];
    assert!(
        database
            .settle_stored("", Ecosystem::Npm, kept)
            .await
            .unwrap()
            .is_empty(),
    );

    let lease = Duration::from_hours(1);
    assert!(
        database
            .stale_reclaims("", Ecosystem::Npm, lease)
            .await
            .unwrap()
            .is_empty(),
    );
    let stale = database.stale_reclaims("", Ecosystem::Npm, Duration::ZERO).await.unwrap();
    assert_eq!(stale, marked);

    let other = vec![BlobRef { id: bytes.clone(), incarnation: 2 }];
    database.finish_reclaims("", Ecosystem::Npm, other).await.unwrap();
    assert_eq!(row(&storage, &bytes).await, Some(reclaiming));
    assert_eq!(storage.finish_stale_reclaims(Duration::ZERO).await.unwrap(), marked);
    let gone = BlobRow { incarnation: 1, state: BlobState::Gone, size: 3 };
    assert_eq!(row(&storage, &bytes).await, Some(gone));
}

conformance! {
    only_live_blobs_nothing_maps_are_marked,
    a_settle_removes_only_what_no_commit_can_make_live,
    a_stopped_reclaim_is_taken_over_after_its_lease,
}

fn fenced(blob: BlobRef) -> Uncommitted {
    Uncommitted { blob, may_fence: true }
}

fn unfenced(blob: BlobRef) -> Uncommitted {
    Uncommitted { blob, may_fence: false }
}
