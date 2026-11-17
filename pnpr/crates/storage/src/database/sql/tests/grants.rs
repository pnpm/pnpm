use super::{Db, storage};
use crate::database::{
    GrantContent, GrantWrite, NewRegistryGraph, RegistryGraphWrite, StoredGrant, TeamRemoval,
};

fn content(name: &str, registries: &[&str], subjects: &[(&str, &str)]) -> GrantContent {
    GrantContent {
        name: name.to_string(),
        registries: registries
            .iter()
            .map(ToString::to_string)
            .collect(),
        include: Vec::new(),
        exclude: Vec::new(),
        subjects: subjects
            .iter()
            .map(|(subject, action)| ((*subject).to_string(), (*action).to_string()))
            .collect(),
    }
}

fn written(write: GrantWrite) -> u64 {
    match write {
        GrantWrite::Written(generation) => generation,
        other => panic!("expected a write, got {other:?}"),
    }
}

async fn another_instance_reads_grants_only_after_they_change(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let empty = second
        .read_grants(None)
        .await
        .unwrap()
        .unwrap();
    assert!(empty.grants.is_empty());
    assert_eq!(
        second
            .read_grants(Some(empty.generation))
            .await
            .unwrap(),
        None
    );

    let mut readers =
        content("readers", &["local", "corp"], &[("$all", "read"), ("u-1", "publish")]);
    readers.include = vec!["@acme/*".to_string()];
    readers.exclude = vec!["@acme/secret".to_string()];
    written(first.put_grant("g-2", readers.clone(), None).await.unwrap());
    let admins = content("admins", &["*"], &[("u-9", "manage")]);
    let generation = written(first.put_grant("g-1", admins.clone(), None).await.unwrap());

    let stored = second
        .read_grants(Some(empty.generation))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.generation, generation);
    readers.registries.sort();
    assert_eq!(
        stored.grants,
        [
            StoredGrant { id: "g-1".to_string(), content: admins },
            StoredGrant { id: "g-2".to_string(), content: readers },
        ],
    );
    assert_eq!(
        second
            .read_grants(Some(generation))
            .await
            .unwrap(),
        None
    );
}

async fn a_grant_write_is_refused_on_a_stale_generation_or_a_taken_name(db: Db) {
    let storage = storage(&db);
    let readers = content("readers", &["local"], &[("$all", "read")]);
    let first = written(storage.put_grant("g-1", readers.clone(), None).await.unwrap());
    assert_eq!(
        storage.put_grant("g-2", readers.clone(), None).await.unwrap(),
        GrantWrite::NameTaken,
    );
    let second = written(
        storage
            .put_grant("g-1", readers.clone(), Some(first))
            .await
            .unwrap(),
    );
    assert_eq!(
        storage
            .put_grant("g-1", readers, Some(first))
            .await
            .unwrap(),
        GrantWrite::Conflict { current: second },
    );
    assert_eq!(
        storage
            .set_grant_subjects("g-1", Vec::new(), Some(first))
            .await
            .unwrap(),
        GrantWrite::Conflict { current: second },
    );
    assert_eq!(
        storage
            .remove_grant("g-1", Some(first))
            .await
            .unwrap(),
        GrantWrite::Conflict { current: second },
    );
    assert_eq!(storage.remove_grant("g-absent", None).await.unwrap(), GrantWrite::NotFound);
    assert_eq!(
        storage
            .set_grant_subjects("g-absent", Vec::new(), None)
            .await
            .unwrap(),
        GrantWrite::NotFound,
    );
    written(
        storage
            .remove_grant("g-1", Some(second))
            .await
            .unwrap(),
    );
    assert!(
        storage
            .read_grants(None)
            .await
            .unwrap()
            .unwrap()
            .grants
            .is_empty()
    );
}

async fn a_grant_and_the_teams_it_names_are_kept_together(db: Db) {
    let storage = storage(&db);
    let team = content("release", &["local"], &[("team:t-release", "publish")]);
    assert_eq!(
        storage.put_grant("g-1", team.clone(), None).await.unwrap(),
        GrantWrite::UnknownTeam("t-release".to_string()),
    );
    storage.create_team("t-release", "release").await.unwrap();
    written(storage.put_grant("g-1", team, None).await.unwrap());
    assert_eq!(storage.remove_team("t-release").await.unwrap(), TeamRemoval::Named);

    let subjects = vec![("team:t-absent".to_string(), "read".to_string())];
    assert_eq!(
        storage.set_grant_subjects("g-1", subjects, None).await.unwrap(),
        GrantWrite::UnknownTeam("t-absent".to_string()),
    );
    let subjects = vec![("$all".to_string(), "read".to_string())];
    written(storage.set_grant_subjects("g-1", subjects.clone(), None).await.unwrap());
    assert_eq!(storage.remove_team("t-release").await.unwrap(), TeamRemoval::Removed);
    let stored = storage
        .read_grants(None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.grants[0].content.subjects, subjects);
}

fn revision<'a>(seeded: &'a [StoredGrant], removed: &'a [String]) -> NewRegistryGraph<'a> {
    NewRegistryGraph {
        document: "{}",
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
        seeded,
        removed,
    }
}

async fn a_graph_write_seeds_and_forgets_the_grants_of_its_registries(db: Db) {
    let storage = storage(&db);
    let seed = |id: &str, name: &str| StoredGrant {
        id: id.to_string(),
        content: content(name, &[name], &[("$all", "read")]),
    };
    let names = |grants: crate::database::StoredGrants| {
        grants.grants
            .into_iter()
            .map(|grant| (grant.content.name, grant.content.registries))
            .collect::<Vec<_>>()
    };
    let seeded = [seed("g-1", "local"), seed("g-2", "corp")];
    assert_eq!(
        storage
            .write_registry_graph(None, revision(&seeded, &[]))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(1),
    );
    let shared = content("shared", &["corp", "local"], &[("$authenticated", "read")]);
    written(storage.put_grant("g-3", shared, None).await.unwrap());

    // A seed whose name is taken keeps the grant an admin may have edited.
    let again = [seed("g-9", "local")];
    let removed = ["corp".to_string()];
    assert_eq!(
        storage
            .write_registry_graph(Some(1), revision(&again, &removed))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(2),
    );
    let stored = storage
        .read_grants(None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.grants[0].id, "g-1");
    assert_eq!(
        names(stored),
        [
            ("local".to_string(), vec!["local".to_string()]),
            ("shared".to_string(), vec!["local".to_string()]),
        ],
    );
}

conformance!(
    another_instance_reads_grants_only_after_they_change,
    a_grant_write_is_refused_on_a_stale_generation_or_a_taken_name,
    a_grant_and_the_teams_it_names_are_kept_together,
    a_graph_write_seeds_and_forgets_the_grants_of_its_registries,
);
