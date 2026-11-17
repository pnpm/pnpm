use super::{Db, storage};
use crate::database::{
    NewRegistryGraph, RegistryGraphWrite, StoredTeam, TeamCreation, TeamRemoval,
};

fn team(id: &str, name: &str, members: &[&str]) -> StoredTeam {
    StoredTeam {
        id: id.to_string(),
        name: name.to_string(),
        members: members
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

async fn another_instance_reads_teams_only_after_they_change(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let empty = second
        .read_teams(None)
        .await
        .unwrap()
        .unwrap();
    assert!(empty.teams.is_empty());
    assert_eq!(
        second
            .read_teams(Some(empty.generation))
            .await
            .unwrap(),
        None,
    );

    assert_eq!(first.create_team("t-release", "release").await.unwrap(), TeamCreation::Created);
    assert_eq!(first.create_team("t-docs", "docs").await.unwrap(), TeamCreation::Created);
    assert_eq!(first.create_team("t-other", "docs").await.unwrap(), TeamCreation::NameTaken);
    assert!(first.add_team_member("t-release", "u-2").await.unwrap());
    assert!(first.add_team_member("t-release", "u-1").await.unwrap());
    assert!(first.add_team_member("t-docs", "u-1").await.unwrap());
    assert!(!first.add_team_member("t-absent", "u-1").await.unwrap());

    let stored = second
        .read_teams(Some(empty.generation))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.teams,
        [team("t-docs", "docs", &["u-1"]), team("t-release", "release", &["u-1", "u-2"])],
    );
    assert_eq!(
        second
            .read_teams(Some(stored.generation))
            .await
            .unwrap(),
        None,
    );
}

async fn a_write_that_changes_nothing_keeps_the_generation(db: Db) {
    let storage = storage(&db);
    storage.create_team("t-release", "release").await.unwrap();
    assert!(storage.add_team_member("t-release", "u-1").await.unwrap());
    let generation = storage
        .read_teams(None)
        .await
        .unwrap()
        .unwrap()
        .generation;

    assert!(storage.add_team_member("t-release", "u-1").await.unwrap());
    assert!(storage.remove_team_member("t-release", "u-absent").await.unwrap());
    storage.remove_team_memberships("u-absent").await.unwrap();
    assert_eq!(storage.remove_team("t-absent").await.unwrap(), TeamRemoval::NotFound);
    assert_eq!(
        storage
            .read_teams(Some(generation))
            .await
            .unwrap(),
        None,
    );
}

async fn removals_take_the_memberships_with_them(db: Db) {
    let storage = storage(&db);
    for (id, name) in [("t-release", "release"), ("t-docs", "docs")] {
        storage.create_team(id, name).await.unwrap();
        for member in ["u-1", "u-2"] {
            assert!(storage.add_team_member(id, member).await.unwrap());
        }
    }
    let teams = || async {
        storage
            .read_teams(None)
            .await
            .unwrap()
            .unwrap()
            .teams
    };

    assert!(storage.remove_team_member("t-docs", "u-2").await.unwrap());
    assert!(!storage.remove_team_member("t-absent", "u-2").await.unwrap());
    storage.remove_team_memberships("u-1").await.unwrap();
    assert_eq!(
        teams().await,
        [team("t-docs", "docs", &[]), team("t-release", "release", &["u-2"])],
    );

    assert_eq!(storage.remove_team("t-release").await.unwrap(), TeamRemoval::Removed);
    assert_eq!(teams().await, [team("t-docs", "docs", &[])]);
    // A team created under the removed one's ID starts without its members.
    storage.create_team("t-release", "release").await.unwrap();
    assert_eq!(teams().await, [team("t-docs", "docs", &[]), team("t-release", "release", &[])]);
}

/// A graph revision whose access lists start to name the team `id`.
fn naming<'a>(document: &'a str, teams: &'a [String]) -> NewRegistryGraph<'a> {
    NewRegistryGraph {
        document,
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
        teams,
    }
}

async fn a_graph_and_the_teams_it_names_are_kept_together(db: Db) {
    let storage = storage(&db);
    let document = r#"{"registries":{"local":{"type":"hosted","access":["team:t-release"]}}}"#;
    let teams = ["t-release".to_string()];
    assert_eq!(
        storage
            .write_registry_graph(None, naming(document, &teams))
            .await
            .unwrap(),
        RegistryGraphWrite::UnknownTeam("t-release".to_string()),
    );
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), None);

    assert_eq!(
        storage
            .unknown_team(&teams)
            .await
            .unwrap()
            .as_deref(),
        Some("t-release"),
    );
    storage.create_team("t-release", "release").await.unwrap();
    storage.create_team("t-release-2", "release-2").await.unwrap();
    assert_eq!(storage.unknown_team(&teams).await.unwrap(), None);
    assert_eq!(
        storage
            .write_registry_graph(None, naming(document, &teams))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(1),
    );
    assert_eq!(storage.remove_team("t-release").await.unwrap(), TeamRemoval::Named);
    // An ID that only begins like the named one is free to go.
    assert_eq!(storage.remove_team("t-release-2").await.unwrap(), TeamRemoval::Removed);

    assert_eq!(
        storage
            .write_registry_graph(Some(1), naming("{}", &[]))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(2),
    );
    assert_eq!(storage.remove_team("t-release").await.unwrap(), TeamRemoval::Removed);
}

conformance!(
    a_graph_and_the_teams_it_names_are_kept_together,
    another_instance_reads_teams_only_after_they_change,
    a_write_that_changes_nothing_keeps_the_generation,
    removals_take_the_memberships_with_them,
);
