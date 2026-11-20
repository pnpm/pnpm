use super::{
    Db,
    storage,
};
use crate::database::{
    GrantContent,
    GrantWrite,
    NewRegistryGraph,
    RegistryGraphWrite,
    StoredGrant,
    TeamRemoval,
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
        None,
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
        None,
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
            .is_empty(),
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

const LOCAL_AND_CORP: &str = r#"{"registries":{"local":{},"corp":{}}}"#;
const LOCAL_AND_SHARED: &str = r#"{"registries":{"local":{},"shared":{}}}"#;

fn revision<'a>(
    document: &'a str,
    seeded: &'a [StoredGrant],
    removed: &'a [String],
) -> NewRegistryGraph<'a> {
    NewRegistryGraph {
        document,
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
            .write_registry_graph(None, revision(LOCAL_AND_CORP, &seeded, &[]))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(1),
    );
    let shared = content("shared", &["corp", "local"], &[("$authenticated", "read")]);
    written(storage.put_grant("g-3", shared, None).await.unwrap());

    // A seed whose registry the grant of its name covers keeps that grant,
    // which an admin may have edited. A seed whose name another registry's
    // grant has takes the next free name.
    let again = [seed("g-9", "local"), seed("g-8", "shared")];
    let removed = ["corp".to_string()];
    assert_eq!(
        storage
            .write_registry_graph(Some(1), revision(LOCAL_AND_SHARED, &again, &removed))
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
            ("shared-2".to_string(), vec!["shared".to_string()]),
        ],
    );

    // The registry the graph write removed cannot come back into a grant.
    let stale = content("stale", &["corp"], &[("$all", "read")]);
    assert_eq!(
        storage.put_grant("g-4", stale.clone(), None).await.unwrap(),
        GrantWrite::UnknownRegistry("corp".to_string()),
    );
    let stale = vec![StoredGrant { id: "g-4".to_string(), content: stale }];
    assert_eq!(
        storage.replace_grants(stale, None).await.unwrap(),
        GrantWrite::UnknownRegistry("corp".to_string()),
    );
    let every = content("every", &["*"], &[("$authenticated", "read")]);
    written(storage.put_grant("g-5", every, None).await.unwrap());

    // A grant of the registry's name on every registry covers it too.
    let named = [seed("g-7", "every")];
    let document = r#"{"registries":{"local":{},"shared":{},"every":{}}}"#;
    assert_eq!(
        storage
            .write_registry_graph(Some(2), revision(document, &named, &[]))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(3),
    );
    let stored = storage
        .read_grants(None)
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored.grants
            .iter()
            .all(|grant| grant.id != "g-7"),
        "{stored:?}",
    );
}

async fn a_restored_graph_revision_never_seeds_grants(db: Db) {
    let storage = storage(&db);
    let seed = |id: &str, name: &str| StoredGrant {
        id: id.to_string(),
        content: content(name, &[name], &[("$all", "read")]),
    };
    let initial = [seed("g-1", "local"), seed("g-2", "corp")];
    assert_eq!(
        storage
            .write_registry_graph(None, revision(LOCAL_AND_CORP, &initial, &[]))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(1),
    );
    let removed = ["corp".to_string()];
    let added = [seed("g-3", "shared")];
    assert_eq!(
        storage
            .write_registry_graph(Some(1), revision(LOCAL_AND_SHARED, &added, &removed))
            .await
            .unwrap(),
        RegistryGraphWrite::Written(2),
    );

    let unseeded = [seed("g-4", "corp")];
    let forgotten = ["shared".to_string()];
    let restored = NewRegistryGraph {
        restored_from: Some(1),
        ..revision(LOCAL_AND_CORP, &unseeded, &forgotten)
    };
    assert_eq!(
        storage
            .write_registry_graph(Some(2), restored)
            .await
            .unwrap(),
        RegistryGraphWrite::Written(3),
    );
    let stored = storage
        .read_grants(None)
        .await
        .unwrap()
        .unwrap();
    let names: Vec<String> = stored.grants
        .into_iter()
        .map(|grant| grant.content.name)
        .collect();
    assert_eq!(names, ["local"]);
}

conformance!(
    another_instance_reads_grants_only_after_they_change,
    a_grant_write_is_refused_on_a_stale_generation_or_a_taken_name,
    a_grant_and_the_teams_it_names_are_kept_together,
    a_graph_write_seeds_and_forgets_the_grants_of_its_registries,
    a_restored_graph_revision_never_seeds_grants,
);
