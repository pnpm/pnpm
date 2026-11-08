//! Atomic publications, their recorded outcomes, and publication lookups.

use super::{Db, npm, packument, read, schema::execute, storage, write};
use crate::{
    metadata::{
        BatchDocument, PublicationCommit, PublicationRequest, PublishNotes, ReleaseRecords,
        SqlMetadata,
        sql::{adapter::ReadSource, schema},
    },
    publish::now_millis,
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn document(name: &str, document: &Value) -> BatchDocument {
    BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(document).unwrap(),
        expected: None,
        principal: Some("alice".to_string()),
        records: ReleaseRecords::new(),
    }
}

fn request(digest: &str, name: Option<&str>) -> PublicationRequest {
    PublicationRequest {
        principal: "alice".to_string(),
        idempotency_key: format!("request:{digest}"),
        request_digest: digest.to_string(),
        name: name.map(str::to_string),
    }
}

async fn commit(
    database: &Arc<SqlMetadata>,
    documents: Vec<BatchDocument>,
    request: PublicationRequest,
) -> Result<PublicationCommit, RegistryError> {
    database.commit_publication(
        "acme",
        documents,
        Vec::new(),
        PublishNotes::default(),
        Some(request),
    )
    .await
}

async fn a_batch_commits_every_package_under_one_generation_or_none(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let core = packument("core", &["2.0.0"]);
    let widgets = packument("widgets", &["2.0.0"]);
    let batch = vec![document("core", &core), document("widgets", &widgets)];
    let committed = commit(database, batch, request("one", Some("v2"))).await.unwrap();
    assert_eq!(committed.generation.value, 1);
    assert_eq!(committed.publication, Some(1));
    assert_eq!(read(&org, "core").await, core);
    assert_eq!(read(&org, "widgets").await, widgets);

    // A batch whose second member rewrites an accepted version commits none
    // of its members.
    let mut changed = packument("widgets", &["2.0.0"]);
    changed["versions"]["2.0.0"]["description"] = json!("changed");
    let batch =
        vec![document("fresh", &packument("fresh", &["1.0.0"])), document("widgets", &changed)];
    let err = commit(database, batch, request("two", None)).await.unwrap_err();
    assert!(matches!(err, RegistryError::DocumentWriteConflict { .. }), "{err:?}");
    assert!(
        org.read_hosted_document(&npm("fresh")).await
            .unwrap()
            .is_none(),
    );
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 1);
}

async fn a_retried_request_gets_its_recorded_outcome(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let doc = packument("core", &["1.0.0"]);
    let first = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    let retry = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    assert!(retry.replayed);
    assert_eq!((retry.generation, retry.publication), (first.generation, first.publication));
    let recorded = database
        .recorded_outcome("acme", &request("digest", None))
        .await
        .unwrap();
    assert_eq!(recorded.map(|outcome| outcome.generation), Some(1));
    let mut by_bob = request("digest", None);
    by_bob.principal = "bob".to_string();
    // Another principal, or the same request sent to another catalog, is a
    // publish of its own.
    for (catalog, other) in [("acme", by_bob), ("other", request("digest", None))] {
        let found = database.recorded_outcome(catalog, &other).await.unwrap();
        assert!(found.is_none(), "{catalog}");
    }

    let mut reused = request("other", None);
    reused.idempotency_key = "request:digest".to_string();
    let err = commit(database, vec![document("core", &doc)], reused).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn outcome_keys(db: &Db) -> Vec<String> {
    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        let mut keys = Vec::new();
        adapter
            .read(ReadSource::Primary, &mut |connection| {
                keys = connection
                    .query(
                        "SELECT idempotency_key FROM publication_outcomes ORDER BY idempotency_key",
                        params![],
                    )?
                    .into_iter()
                    .map(|mut row| row.get::<String>(0))
                    .collect::<pnpr_error::Result<Vec<_>>>()?;
                Ok(())
            })
            .unwrap();
        keys
    })
    .await
    .unwrap()
}

async fn sweep_query_plan(db: &Db) -> Vec<String> {
    let query = db.sql(
        "EXPLAIN QUERY PLAN DELETE FROM publication_outcomes WHERE committed_at < ?1",
        "EXPLAIN DELETE FROM publication_outcomes WHERE committed_at < ?1",
        "EXPLAIN FORMAT=JSON DELETE FROM publication_outcomes WHERE committed_at < ?1",
    );
    let detail_column = db
        .sql("3", "0", "0")
        .parse::<usize>()
        .unwrap();
    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        let mut details = Vec::new();
        adapter
            .read(ReadSource::Replica, &mut |connection| {
                details = connection
                    .query(query, params![0_i64])?
                    .into_iter()
                    .map(|mut row| {
                        row.get::<Option<String>>(detail_column).map(Option::unwrap_or_default)
                    })
                    .collect::<pnpr_error::Result<Vec<_>>>()?;
                Ok(())
            })
            .unwrap();
        details
    })
    .await
    .unwrap()
}

async fn the_outcome_sweep_uses_the_committed_at_index(db: Db) {
    let storage = storage(&db);
    commit(
        storage.sql_metadata(),
        vec![document("core", &packument("core", &["1.0.0"]))],
        request("one", None),
    )
    .await
    .unwrap();
    let plan = sweep_query_plan(&db).await;
    assert!(
        plan.iter()
            .any(|detail| detail.contains("publication_outcomes_by_committed_at")),
        "{plan:?}",
    );
    assert!(
        !plan
            .iter()
            .any(|detail| detail.contains("SCAN publication_outcomes")),
        "{plan:?}",
    );
}

async fn expired_outcomes_are_swept(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    commit(
        database,
        vec![document("core", &packument("core", &["1.0.0"]))],
        request("stale", None),
    )
    .await
    .unwrap();
    commit(
        database,
        vec![document("widgets", &packument("widgets", &["1.0.0"]))],
        request("retained", None),
    )
    .await
    .unwrap();

    execute(
        &db,
        &["UPDATE publication_outcomes SET committed_at = 0 WHERE idempotency_key = 'request:stale'"],
    )
    .await;
    assert!(
        database
            .recorded_outcome("acme", &request("stale", None))
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(outcome_keys(&db).await, ["request:retained", "request:stale"]);

    commit(
        database,
        vec![document("extra", &packument("extra", &["1.0.0"]))],
        request("fresh", None),
    )
    .await
    .unwrap();
    assert_eq!(outcome_keys(&db).await, ["request:fresh", "request:retained"]);
}

async fn an_unpublished_member_invalidates_the_recorded_outcome(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let core = packument("core", &["1.0.0"]);
    let widgets = packument("widgets", &["1.0.0"]);
    commit(
        database,
        vec![document("core", &core), document("widgets", &widgets)],
        request("one", None),
    )
    .await
    .unwrap();
    assert!(
        org.remove_package(&npm("widgets"), None).await
            .unwrap(),
    );
    assert!(
        database
            .recorded_outcome("acme", &request("one", None))
            .await
            .unwrap()
            .is_none(),
    );

    let mut retained = document("core", &core);
    retained.expected = Some(crate::HostedDocumentVersion::Unversioned);
    let err = commit(database, vec![retained, document("widgets", &widgets)], request("one", None))
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::CannotRepublishUnpublishedVersion { .. }), "{err:?}");
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 2);
    assert_eq!(read(&org, "core").await, core);
    let mut reused = request("different", None);
    reused.idempotency_key = "request:one".to_string();
    let err = commit(database, vec![document("core", &core)], reused).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn publications_are_found_by_id_name_and_member(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let batch = vec![
        document("core", &packument("core", &["1.0.0"])),
        document("widgets", &packument("widgets", &["1.0.0"])),
    ];
    commit(database, batch, request("one", Some("v1"))).await.unwrap();
    let by_name = database
        .publication("acme", "v1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(database.publication("acme", "1").await.unwrap(), Some(by_name.clone()));
    let members: Vec<_> = by_name.members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(members, ["core", "widgets"]);
    assert_eq!(by_name.publisher.as_deref(), Some("alice"));
    assert_eq!(by_name.members[0].artifact, "sha512-1.0.0");
    let initial = database
        .initial_publication(&npm("widgets"), "acme", "1.0.0")
        .await
        .unwrap();
    assert_eq!(initial.map(|publication| publication.id), Some(1));

    let mut taken = document("core", &packument("core", &["1.0.0", "1.1.0"]));
    taken.expected = Some(crate::HostedDocumentVersion::Unversioned);
    let taken = vec![taken];
    let err = commit(database, taken, request("two", Some("v1"))).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn a_version_written_outside_a_publish_request_gets_a_publication(db: Db) {
    let storage = storage(&db);
    write(&storage, "plain", &packument("plain", &["1.0.0"])).await;
    write(&storage, "plain", &packument("plain", &["1.0.0", "1.1.0"])).await;
    let database = storage.sql_metadata();
    let first = database
        .initial_publication(&npm("plain"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.id, first.publisher), (1, None));
    let second = database
        .initial_publication(&npm("plain"), "", "1.1.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.id, 2);
    let versions: Vec<&str> = second.members
        .iter()
        .map(|member| member.version.as_str())
        .collect();
    assert_eq!(versions, ["1.1.0"]);
    assert!(
        database
            .initial_publication(&npm("plain"), "", "9.9.9")
            .await
            .unwrap()
            .is_none(),
    );
}

async fn a_publication_without_a_request_records_its_publisher(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    // No request, as a staged publish's approval commits: the publisher still
    // comes from the document.
    let mut core = document("core", &packument("core", &["1.0.0"]));
    core.principal = Some("carol".to_string());
    database
        .commit_publication("acme", vec![core], Vec::new(), PublishNotes::default(), None)
        .await
        .unwrap();
    let history = database
        .tag_history(&npm("core"), "acme", "latest")
        .await
        .unwrap();
    let principals: Vec<_> = history
        .iter()
        .map(|entry| entry.principal.as_deref())
        .collect();
    assert_eq!(principals, [Some("carol")]);
    let publication = database
        .initial_publication(&npm("core"), "acme", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(publication.publisher.as_deref(), Some("carol"));
}

async fn a_publication_cannot_bring_back_a_removed_version(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let batch = vec![document("core", &packument("core", &["1.0.0"]))];
    commit(database, batch, request("one", None)).await.unwrap();
    let removed = org
        .remove_package(&npm("core"), None)
        .await
        .unwrap();
    assert!(removed);

    let batch = vec![document("core", &packument("core", &["1.0.0", "2.0.0"]))];
    let err = commit(database, batch, request("two", None)).await.unwrap_err();
    assert!(matches!(err, RegistryError::CannotRepublishUnpublishedVersion { .. }), "{err:?}");
    assert!(
        org.read_hosted_document(&npm("core")).await
            .unwrap()
            .is_none(),
    );
}

async fn competing_publications_of_one_version_commit_once(db: Db) {
    // Two stores over one database: two writers that share no in-process lock.
    let (first, second) = (storage(&db), storage(&db));
    let mut theirs = packument("core", &["1.0.0"]);
    theirs["versions"]["1.0.0"]["dist"]["integrity"] = json!("sha512-theirs");
    let ours = vec![
        document("core", &packument("core", &["1.0.0"])),
        document("extra", &packument("extra", &["1.0.0"])),
    ];
    let theirs = vec![document("core", &theirs)];
    let left = commit(first.sql_metadata(), ours, request("ours", None));
    let right = commit(second.sql_metadata(), theirs, request("theirs", None));
    let (left, right) = tokio::join!(left, right);
    let winner = match (left, right) {
        (Ok(won), Err(RegistryError::DocumentWriteConflict { .. })) => {
            assert!(read(&first.for_hosted("acme", Ecosystem::Npm), "extra").await.is_object());
            won
        }
        (Err(RegistryError::DocumentWriteConflict { .. }), Ok(won)) => {
            // The losing batch committed none of its members.
            let extra = first
                .for_hosted("acme", Ecosystem::Npm)
                .read_hosted_document(&npm("extra"))
                .await
                .unwrap();
            assert!(extra.is_none());
            won
        }
        outcomes => panic!("exactly one publication commits, got {outcomes:?}"),
    };
    assert_eq!(winner.generation.value, 1);
    let database = first.sql_metadata();
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 1);
    let second_publication = database.publication("acme", "2").await.unwrap();
    assert!(second_publication.is_none());
}

/// A packument of `name` whose versions were accepted under the npm profile.
fn verified(name: &str, versions: &[&str]) -> Value {
    let mut document = packument(name, versions);
    for version in versions {
        document["versions"][*version]["_archiveManifest"] =
            json!({ "profile": "npm-install-manifest/1", "packageJson": "sha512-input" });
    }
    document
}

/// A release record of `name@version`, revision 0, under the npm profile.
fn record_of(name: &str, version: &str) -> Vec<u8> {
    let record = json!({
        "schema": "pnpr-release-record/1",
        "purl": format!("pkg:npm/{name}@{version}"),
        "name": name,
        "version": version,
        "revision": 0,
        "interpretation": "npm-install-manifest/1",
        "manifest": {},
    });
    serde_json::to_vec(&record).unwrap()
}

async fn a_version_is_accepted_with_its_release_record(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let mut core = document("core", &verified("core", &["1.0.0"]));
    let record = record_of("core", "1.0.0");
    core.records.insert("1.0.0".to_string(), record.clone());
    commit(database, vec![core], request("one", None)).await.unwrap();
    let read_record = async |version, revision| {
        org.read_release_record(&npm("core"), version, revision).await
            .unwrap()
    };
    assert_eq!(read_record("1.0.0", 0).await, Some(record.clone()));
    assert_eq!(read_record("1.0.0", 1).await, None);
    assert_eq!(read_record("2.0.0", 0).await, None);

    // A version the store holds keeps the record it was accepted with.
    let mut restated = document("core", &read(&org, "core").await);
    restated.expected = Some(crate::HostedDocumentVersion::Generation(1));
    restated.records.insert("1.0.0".to_string(), record_of("core", "1.0.0-other"));
    commit(database, vec![restated], request("two", None)).await.unwrap();
    assert_eq!(read_record("1.0.0", 0).await, Some(record));

    // Removing the version removes its record.
    write(&org, "core", &packument("core", &[])).await;
    assert_eq!(read_record("1.0.0", 0).await, None);
}

async fn a_release_record_must_describe_its_version(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    for (version, record) in [
        ("1.0.0", record_of("core", "2.0.0")),
        ("1.0.0", record_of("other", "1.0.0")),
        ("1.0.0", b"{}".to_vec()),
    ] {
        let mut core = document("core", &verified("core", &[version]));
        core.records.insert(version.to_string(), record);
        let err = commit(database, vec![core], request("one", None)).await.unwrap_err();
        assert!(matches!(err, RegistryError::Internal { .. }), "{err:?}");
    }
    assert!(
        org.read_hosted_document(&npm("core")).await
            .unwrap()
            .is_none(),
    );
}

conformance! {
    a_version_is_accepted_with_its_release_record,
    a_release_record_must_describe_its_version,
    a_batch_commits_every_package_under_one_generation_or_none,
    a_retried_request_gets_its_recorded_outcome,
    expired_outcomes_are_swept,
    an_unpublished_member_invalidates_the_recorded_outcome,
    publications_are_found_by_id_name_and_member,
    a_version_written_outside_a_publish_request_gets_a_publication,
    a_publication_without_a_request_records_its_publisher,
    a_publication_cannot_bring_back_a_removed_version,
    competing_publications_of_one_version_commit_once,
}

/// `EXPLAIN QUERY PLAN` is `SQLite` syntax.
mod embedded_query_plan {
    use super::the_outcome_sweep_uses_the_committed_at_index;
    embedded_conformance!(the_outcome_sweep_uses_the_committed_at_index);
}

#[derive(Debug, Clone, Copy)]
enum OutcomeIndex {
    WithoutIndex,
    WithIndex,
}

async fn seed_outcomes(db: &Db, prefix: &str, count: usize, base_millis: i64) {
    if count == 0 {
        return;
    }
    let sql = format!(
        "WITH RECURSIVE series(i) AS (
             SELECT 1
             UNION ALL
             SELECT i + 1 FROM series WHERE i < {count}
         )
         INSERT INTO publication_outcomes (
             catalog, principal, idempotency_key, request_digest,
             generation, publication, committed_at
         )
         SELECT
             'acme',
             'principal-' || (i % 64),
             '{prefix}-' || printf('%06d', i),
             'digest-' || printf('%056d', i),
             i,
             i,
             {base_millis} + i
         FROM series",
    );
    execute(db, &[&sql]).await;
}

async fn measure_outcome_sweep(
    db: Db,
    layout: OutcomeIndex,
    retained: usize,
    expired_per_publish: usize,
) {
    let mut tables = schema::TABLES.to_string();
    if matches!(layout, OutcomeIndex::WithoutIndex) {
        tables = tables.replace(
            "CREATE INDEX IF NOT EXISTS publication_outcomes_by_committed_at\n    ON publication_outcomes (committed_at);\n",
            "",
        );
        assert!(!tables.contains("publication_outcomes_by_committed_at"));
    }
    let mut statements: Vec<_> = schema::statements(&tables).collect();
    statements.push(schema::MARKER_TABLE.to_string());
    statements.push(format!("INSERT INTO schema_version VALUES ('{}')", schema::SCHEMA_VERSION));
    let refs: Vec<_> = statements
        .iter()
        .map(String::as_str)
        .collect();
    execute(&db, &refs).await;
    seed_outcomes(&db, "retained", retained, now_millis()).await;

    let storage = storage(&db);
    let database = storage.sql_metadata();
    let mut times = Vec::new();
    for sample in 0..16 {
        let prefix = format!("expired-{sample}");
        seed_outcomes(&db, &prefix, expired_per_publish, 1).await;
        let name = format!("pkg-{sample}");
        let batch = vec![document(&name, &packument(&name, &["1.0.0"]))];
        let req = request(&name, None);
        let started = Instant::now();
        commit(database, batch, req).await.unwrap();
        let elapsed = started.elapsed();
        if sample > 0 {
            times.push(elapsed);
        }
    }
    assert_eq!(outcome_keys(&db).await.len(), retained + 16);
    times.sort();
    let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
    eprintln!(
        "{layout:?} (retained={retained}, expired_per_publish={expired_per_publish}): median {:.2} ms, min {:.2} ms, max {:.2} ms ({} publishes)",
        milliseconds(times[times.len() / 2]),
        milliseconds(times[0]),
        milliseconds(times[times.len() - 1]),
        times.len(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual outcome-retention sweep benchmark"]
async fn outcome_retention_sweep_on_sqlite() {
    for (retained, expired) in [(10_000, 0), (10_000, 100), (50_000, 0), (50_000, 500)] {
        for layout in [OutcomeIndex::WithoutIndex, OutcomeIndex::WithIndex] {
            measure_outcome_sweep(Db::sqlite(), layout, retained, expired).await;
        }
    }
}

#[cfg(feature = "backend-libsql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual outcome-retention sweep benchmark"]
async fn outcome_retention_sweep_on_libsql_primary() {
    for (retained, expired) in [(10_000, 0), (10_000, 100), (50_000, 0), (50_000, 500)] {
        for layout in [OutcomeIndex::WithoutIndex, OutcomeIndex::WithIndex] {
            measure_outcome_sweep(Db::libsql(false).await, layout, retained, expired).await;
        }
    }
}
