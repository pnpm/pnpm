//! Stored revisions of the registry graph.

use super::{Db, packument, storage, try_write, write};
use crate::{
    Storage,
    metadata::{
        HostedNamespace, NewRegistryGraph, REGISTRY_GRAPH_REVISIONS_KEPT, RegistryGraphRevision,
        RegistryGraphWrite,
    },
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;

fn revision(document: &str) -> NewRegistryGraph<'_> {
    NewRegistryGraph {
        document,
        actor_id: "u-admin",
        restored_from: None,
        vacated: &[],
        bound: &[],
    }
}

async fn write_graph(storage: &Storage, expected: Option<u64>, document: &str) -> u64 {
    match storage
        .write_registry_graph(expected, revision(document))
        .await
        .unwrap()
    {
        RegistryGraphWrite::Written(generation) => generation,
        refused => panic!("{refused:?}"),
    }
}

async fn a_written_graph_reads_back_as_the_current_revision(db: Db) {
    let storage = storage(&db);
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), None);
    assert_eq!(storage.read_registry_graph().await.unwrap(), None);
    let before = crate::publish::now_millis();
    let first = write_graph(&storage, None, r#"{"registries":{}}"#).await;
    let second = write_graph(&storage, Some(first), r#"{"registries":{"a":{}}}"#).await;
    assert_eq!((first, second), (1, 2));
    let other_instance = super::storage(&db);
    assert_eq!(other_instance.read_registry_graph_generation().await.unwrap(), Some(2));
    let current = other_instance
        .read_registry_graph()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.generation, 2);
    assert_eq!(current.document, r#"{"registries":{"a":{}}}"#);
    assert_eq!(current.actor_id, "u-admin");
    assert_eq!(current.restored_from, None);
    assert!((before..=crate::publish::now_millis()).contains(&current.created_at));
    let older = other_instance
        .read_registry_graph_revision(1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(older.document, r#"{"registries":{}}"#);
    assert_eq!(other_instance.read_registry_graph_revision(3).await.unwrap(), None);
}

async fn a_write_from_a_stale_read_conflicts(db: Db) {
    let storage = storage(&db);
    let first = write_graph(&storage, None, "{}").await;
    assert_eq!(
        storage
            .write_registry_graph(None, revision("{}"))
            .await
            .unwrap(),
        RegistryGraphWrite::Conflict { current: Some(first) },
    );
    let second = write_graph(&storage, Some(first), "{}").await;
    assert_eq!(
        storage
            .write_registry_graph(Some(first), revision(r#"{"lost":true}"#))
            .await
            .unwrap(),
        RegistryGraphWrite::Conflict { current: Some(second) },
    );
    assert_eq!(
        storage
            .write_registry_graph(Some(7), revision("{}"))
            .await
            .unwrap(),
        RegistryGraphWrite::Conflict { current: Some(second) },
    );
    assert_eq!(
        storage
            .read_registry_graph()
            .await
            .unwrap()
            .unwrap()
            .document,
        "{}",
    );
}

async fn an_empty_database_rejects_an_expected_generation(db: Db) {
    let storage = storage(&db);
    assert_eq!(
        storage
            .write_registry_graph(Some(1), revision("{}"))
            .await
            .unwrap(),
        RegistryGraphWrite::Conflict { current: None },
    );
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), None);
}

async fn revisions_list_newest_first_in_pages(db: Db) {
    let storage = storage(&db);
    let mut generation = None;
    for _ in 0..5 {
        generation = Some(write_graph(&storage, generation, "{}").await);
    }
    let restored = storage
        .write_registry_graph(
            generation,
            NewRegistryGraph {
                document: "{}",
                actor_id: "-",
                restored_from: Some(2),
                vacated: &[],
                bound: &[],
            },
        )
        .await
        .unwrap();
    assert_eq!(restored, RegistryGraphWrite::Written(6));
    let generations = |page: &[RegistryGraphRevision]| {
        page.iter()
            .map(|revision| revision.generation)
            .collect::<Vec<_>>()
    };
    let first_page = storage.list_registry_graph_revisions(None, 4).await.unwrap();
    assert_eq!(generations(&first_page), [6, 5, 4, 3]);
    assert_eq!(first_page[0].restored_from, Some(2));
    assert_eq!(first_page[0].actor_id, "-");
    assert_eq!(first_page[1].restored_from, None);
    let second_page = storage
        .list_registry_graph_revisions(Some(3), 4)
        .await
        .unwrap();
    assert_eq!(generations(&second_page), [2, 1]);
    let stored = storage
        .read_registry_graph()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.restored_from, Some(2));
}

async fn a_write_keeps_only_the_newest_revisions(db: Db) {
    let storage = storage(&db);
    let mut generation = None;
    for _ in 0..REGISTRY_GRAPH_REVISIONS_KEPT + 2 {
        generation = Some(write_graph(&storage, generation, "{}").await);
    }
    let newest = REGISTRY_GRAPH_REVISIONS_KEPT + 2;
    assert_eq!(generation, Some(newest));
    let limit = u32::try_from(REGISTRY_GRAPH_REVISIONS_KEPT * 2).unwrap();
    let kept = storage.list_registry_graph_revisions(None, limit).await.unwrap();
    assert_eq!(u64::try_from(kept.len()).unwrap(), REGISTRY_GRAPH_REVISIONS_KEPT);
    assert_eq!(kept.first().map(|revision| revision.generation), Some(newest));
    assert_eq!(kept.last().map(|revision| revision.generation), Some(3));
    assert_eq!(storage.read_registry_graph_revision(2).await.unwrap(), None);
}

async fn a_graph_write_invalidates_every_cdn_response(db: Db) {
    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();
    write_graph(&storage, None, "{}").await;
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
}

async fn a_hosted_namespace_holds_packages_once_one_is_written(db: Db) {
    let storage = storage(&db);
    assert!(!storage.hosted_namespace_has_packages("acme", Ecosystem::Npm).await.unwrap());
    let acme = storage.for_hosted("acme", Ecosystem::Npm);
    write(&acme, "widget", &packument("widget", &["1.0.0"])).await;
    assert!(storage.hosted_namespace_has_packages("acme", Ecosystem::Npm).await.unwrap());
    assert!(!storage.hosted_namespace_has_packages("acme", Ecosystem::Cargo).await.unwrap());
    assert!(!storage.hosted_namespace_has_packages("", Ecosystem::Npm).await.unwrap());
}

async fn a_write_that_vacates_an_occupied_namespace_stores_nothing(db: Db) {
    let storage = storage(&db);
    let first = write_graph(&storage, None, r#"{"registries":{}}"#).await;
    let acme = storage.for_hosted("acme", Ecosystem::Npm);
    write(&acme, "widget", &packument("widget", &["1.0.0"])).await;
    let vacated = |namespace: &str, ecosystem| HostedNamespace {
        namespace: namespace.to_string(),
        ecosystem,
    };
    let occupied = [vacated("empty", Ecosystem::Npm), vacated("acme", Ecosystem::Npm)];
    let refused = NewRegistryGraph { vacated: &occupied, ..revision("{}") };
    assert_eq!(
        storage
            .write_registry_graph(Some(first), refused)
            .await
            .unwrap(),
        RegistryGraphWrite::HostedNotEmpty(vacated("acme", Ecosystem::Npm)),
    );
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(first));

    let empty = [vacated("acme", Ecosystem::Cargo), vacated("empty", Ecosystem::Npm)];
    let allowed = NewRegistryGraph { vacated: &empty, ..revision("{}") };
    assert_eq!(
        storage
            .write_registry_graph(Some(first), allowed)
            .await
            .unwrap(),
        RegistryGraphWrite::Written(first + 1),
    );
}

async fn a_vacated_namespace_refuses_publishes_until_a_revision_binds_it_again(db: Db) {
    let retired = [HostedNamespace { namespace: "retired".to_string(), ecosystem: Ecosystem::Npm }];
    let api_managed = storage(&db);
    let first = write_graph(&api_managed, None, "{}").await;
    let vacating = NewRegistryGraph { vacated: &retired, ..revision("{}") };
    let retiring = api_managed
        .write_registry_graph(Some(first), vacating)
        .await
        .unwrap();
    assert_eq!(retiring, RegistryGraphWrite::Written(first + 1));

    let fenced = api_managed.for_hosted("retired", Ecosystem::Npm);
    let refused = try_write(&fenced, "widget", &packument("widget", &["1.0.0"])).await;
    assert!(matches!(refused, Err(RegistryError::RegistriesUnavailable)), "{refused:?}");
    let other_namespace = api_managed.for_hosted("served", Ecosystem::Npm);
    write(&other_namespace, "widget", &packument("widget", &["1.0.0"])).await;

    let binding = NewRegistryGraph { bound: &retired, ..revision("{}") };
    let rebound = api_managed
        .write_registry_graph(Some(first + 1), binding)
        .await
        .unwrap();
    assert_eq!(rebound, RegistryGraphWrite::Written(first + 2));
    write(&fenced, "widget", &packument("widget", &["1.0.0"])).await;
}

conformance!(
    a_vacated_namespace_refuses_publishes_until_a_revision_binds_it_again,
    a_write_that_vacates_an_occupied_namespace_stores_nothing,
    a_written_graph_reads_back_as_the_current_revision,
    a_write_from_a_stale_read_conflicts,
    an_empty_database_rejects_an_expected_generation,
    revisions_list_newest_first_in_pages,
    a_write_keeps_only_the_newest_revisions,
    a_graph_write_invalidates_every_cdn_response,
    a_hosted_namespace_holds_packages_once_one_is_written,
);
