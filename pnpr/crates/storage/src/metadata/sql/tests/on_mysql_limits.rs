use super::{Db, storage};
use crate::metadata::StoredTeam;
use std::collections::BTreeSet;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn oversized_keys_fail_without_truncating_or_advancing_generation() {
    let db = Db::network("PNPR_TEST_MYSQL_URL").await;
    let storage = storage(&db);
    let boundary = "x".repeat(512);
    let original_team = [StoredTeam {
        id: "t-1".to_string(),
        name: "original".to_string(),
        members: BTreeSet::new(),
    }];
    let original = storage.replace_team_roster(&boundary, &original_team).await.unwrap();
    assert!(
        storage
            .replace_team_roster(&format!("{boundary}x"), &[])
            .await
            .is_err(),
    );
    assert_eq!(
        storage
            .read_team_roster(&boundary)
            .await
            .unwrap()
            .unwrap()
            .teams,
        original_team,
    );
    let next = storage
        .replace_team_roster("next", &[])
        .await
        .unwrap();
    assert_eq!(next.generation, original.generation + 1);
}
