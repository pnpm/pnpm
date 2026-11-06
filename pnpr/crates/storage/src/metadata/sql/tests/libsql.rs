//! The libSQL adapter against a local primary that serves libsql's sync
//! protocol: which reads go to the replica, which to the primary, and what a
//! replica that lags the primary may serve.

use super::{npm, packument};
use crate::{
    DocumentWrite, HostedDocumentVersion,
    metadata::{NoteChange, PackageMetadata, SqlMetadata},
};
use pnpr_config::LibsqlSettings;
use pnpr_libsql::testing::{Primary, serve_primary};
use pnpr_package_name::Ecosystem;
use serde_json::Value;
use std::{net::SocketAddr, path::Path, sync::Arc};
use tempfile::TempDir;

/// A primary on a loopback port, and the directory its files live in.
pub(super) struct Server {
    primary: Arc<Primary>,
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    dir: TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn server() -> Server {
    let dir = TempDir::new().unwrap();
    let (primary, address, task) = serve_primary(dir.path().join("primary.db")).await;
    Server { primary, address, task, dir }
}

impl Server {
    /// The metadata of one pnpr instance on this primary: reading through an
    /// embedded replica at `replica` that syncs every `sync_interval_secs`,
    /// or straight from the primary without one.
    pub(super) fn instance(
        &self,
        replica: Option<&str>,
        sync_interval_secs: u64,
    ) -> Arc<SqlMetadata> {
        Arc::new(SqlMetadata::open_libsql(&self.settings(replica, sync_interval_secs)))
    }

    /// The settings of an instance on this primary, as [`Self::instance`]
    /// opens it.
    pub(super) fn settings(
        &self,
        replica: Option<&str>,
        sync_interval_secs: u64,
    ) -> LibsqlSettings {
        LibsqlSettings {
            url: format!("http://{}", self.address),
            auth_token: None,
            replica_path: replica.map(|name| {
                self.dir
                    .path()
                    .join(name)
                    .join("replica.db")
            }),
            sync_interval_secs: Some(sync_interval_secs),
            max_connections: None,
        }
    }

    fn statements(&self) -> usize {
        self.primary.round_trips.count()
    }
}

fn views(database: &Arc<SqlMetadata>) -> PackageMetadata {
    database.view("", Ecosystem::Npm)
}

async fn publish(metadata: &PackageMetadata, versions: &[&str]) {
    let current = metadata
        .read_for_update(&npm("pkg"))
        .await
        .unwrap();
    let bytes = serde_json::to_vec(&packument("pkg", versions)).unwrap();
    let version = current.map(|document| document.version);
    let written = metadata
        .write_if_current(&npm("pkg"), &bytes, version.as_ref(), None)
        .await
        .unwrap();
    assert_eq!(written, DocumentWrite::Written);
}

async fn latest(metadata: &PackageMetadata) -> Option<String> {
    let view = metadata
        .read_view(&npm("pkg"))
        .await
        .unwrap()?;
    let document: Value = serde_json::from_slice(&view.bytes).unwrap();
    document["dist-tags"]["latest"].as_str().map(str::to_string)
}

fn replica_exists(server: &Server, name: &str) -> bool {
    Path::new(
        &server.dir
            .path()
            .join(name)
            .join("replica.db"),
    )
    .is_file()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_primary_alone_keeps_metadata_for_every_instance() {
    let server = server().await;
    let (first, second) = (server.instance(None, 60), server.instance(None, 60));
    publish(&views(&first), &["1.0.0"]).await;
    assert_eq!(latest(&views(&second)).await.as_deref(), Some("1.0.0"));
    let current = views(&second)
        .read_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(current.version, HostedDocumentVersion::Generation(1)), "{current:?}");
    assert_eq!(second.catalog_generation("").await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_serves_views_and_lags_within_its_sync_interval() {
    let server = server().await;
    let writer = views(&server.instance(None, 60));
    publish(&writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 3600));
    assert_eq!(latest(&reader).await.as_deref(), Some("1.0.0"), "bootstrapped from the primary");
    assert!(replica_exists(&server, "reader"));

    let before = server.statements();
    assert_eq!(latest(&reader).await.as_deref(), Some("1.0.0"));
    assert_eq!(server.statements(), before, "a view is read from the replica");

    publish(&writer, &["1.0.0", "2.0.0"]).await;
    assert_eq!(
        latest(&reader).await.as_deref(),
        Some("1.0.0"),
        "another instance's commit reaches the replica at its next sync",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_that_must_be_current_does_not_take_the_replica_copy() {
    let server = server().await;
    let writer = views(&server.instance(None, 60));
    publish(&writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 3600));
    assert!(latest(&reader).await.is_some());
    publish(&writer, &["1.0.0", "2.0.0"]).await;

    let current = reader
        .read_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(current.version, HostedDocumentVersion::Generation(2)),
        "read from the primary: {current:?}",
    );

    let view = reader
        .read_view_at_least(&npm("pkg"), 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.generation.value, 2);
    assert_eq!(
        latest(&reader).await.as_deref(),
        Some("2.0.0"),
        "the replica synced past the generation the read asked for",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_instance_reads_its_own_commits_from_its_replica() {
    let server = server().await;
    let database = server.instance(Some("own"), 3600);
    let metadata = views(&database);
    publish(&metadata, &["1.0.0"]).await;
    publish(&metadata, &["1.0.0", "2.0.0"]).await;
    assert_eq!(latest(&metadata).await.as_deref(), Some("2.0.0"));
    assert_eq!(database.catalog_generation("").await.unwrap(), 2);
    // A note edit advances no generation, and is still read back.
    let change = NoteChange { text: Some("edited\n".to_string()), principal: "bob".to_string() };
    assert!(
        database
            .set_version_note(&npm("pkg"), "", "2.0.0", change)
            .await
            .unwrap(),
    );
    let note = database
        .version_note(&npm("pkg"), "", "2.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(note.text.as_deref(), Some("edited\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_past_its_sync_interval_syncs_before_it_serves() {
    let server = server().await;
    let writer = views(&server.instance(None, 60));
    publish(&writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 1));
    assert!(latest(&reader).await.is_some());
    publish(&writer, &["1.0.0", "2.0.0"]).await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(latest(&reader).await.as_deref(), Some("2.0.0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_that_overtakes_a_sync_leaves_the_replica_stale() {
    use std::sync::atomic::Ordering;
    let server = server().await;
    let metadata = views(&server.instance(Some("own"), 3600));
    // The write leaves the replica stale, so the next view syncs it.
    publish(&metadata, &["1.0.0"]).await;
    server.primary.sync_stall.enabled.store(true, Ordering::SeqCst);
    let reader = metadata.clone();
    let read = tokio::spawn(async move { latest(&reader).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.primary.sync_stall.started.notified(),
    )
    .await
    .expect("the view's sync reaches the primary");
    // Committed while the view's pull answers with the primary as it was.
    publish(&metadata, &["1.0.0", "2.0.0"]).await;
    read.await.unwrap();
    server.primary.sync_stall.enabled.store(false, Ordering::SeqCst);
    assert_eq!(latest(&metadata).await.as_deref(), Some("2.0.0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_storage_of_a_process_shares_one_database_handle() {
    let server = server().await;
    let settings = LibsqlSettings {
        url: format!("http://{}", server.address),
        auth_token: None,
        replica_path: Some(
            server.dir
                .path()
                .join("shared")
                .join("replica.db"),
        ),
        sync_interval_secs: Some(60),
        max_connections: None,
    };
    let config = pnpr_config::StorageConfig {
        hosted_dir: server.dir.path().join("storage"),
        cache_dir: server.dir.path().join("cache"),
        cache: pnpr_config::CacheConfig::default(),
        hosted_backend: pnpr_config::HostedStoreConfig::Fs,
        metadata: pnpr_config::MetadataBackend::Libsql(settings),
    };
    let first = crate::Storage::from_config(&config).unwrap();
    let second = crate::Storage::from_config(&config).unwrap();
    assert!(Arc::ptr_eq(first.sql_metadata(), second.sql_metadata()));
}

/// The round trips each operation since the last call made to the primary,
/// without the `COMMIT` that ends a read of the primary, which is sent after
/// the read answers.
fn round_trips(server: &Server) -> Vec<Vec<String>> {
    server.primary.round_trips
        .take()
        .into_iter()
        .filter(|statements| statements != &["COMMIT".to_string()])
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_makes_two_round_trips_however_much_it_carries() {
    use crate::metadata::{
        BatchDocument, PublicationRequest, PublishNotes, ReleaseRecords, TagChange, VersionNote,
    };
    let server = server().await;
    let database = server.instance(None, 60);
    database.catalog_generation("").await.unwrap();
    let document = |name: &str, versions: &[&str]| {
        let mut document = packument(name, versions);
        document["dist-tags"]["next"] = Value::from(versions[0]);
        document["dist-tags"]["beta"] = Value::from(versions[1]);
        BatchDocument {
            ecosystem: Ecosystem::Npm,
            name: name.to_string(),
            bytes: serde_json::to_vec(&document).unwrap(),
            expected: None,
            principal: Some("alice".to_string()),
            records: ReleaseRecords::new(),
        }
    };
    let names = ["core", "widgets", "tools"];
    let versions = ["1.0.0", "1.1.0", "1.2.0", "2.0.0", "2.1.0"];
    let notes = PublishNotes {
        publication: Some("the first release\n".to_string()),
        versions: names
            .iter()
            .map(|name| VersionNote {
                ecosystem: Ecosystem::Npm,
                name: (*name).to_string(),
                version: "2.1.0".to_string(),
                text: "notes\n".to_string(),
            })
            .collect(),
    };
    let request = PublicationRequest {
        principal: "alice".to_string(),
        idempotency_key: "request:one".to_string(),
        request_digest: "one".to_string(),
        name: Some("first".to_string()),
    };
    round_trips(&server);
    let documents = names
        .iter()
        .map(|name| document(name, &versions))
        .collect();
    let committed = database
        .commit_publication("", documents, Vec::new(), notes, Some(request.clone()))
        .await
        .unwrap();
    assert_eq!(committed.publication, Some(1));
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a publication of 3 packages of 5 versions each: {made:#?}");

    // A retry finds its outcome in the round trip that takes the lock.
    let replayed = database
        .commit_publication("", Vec::new(), Vec::new(), PublishNotes::default(), Some(request))
        .await
        .unwrap();
    assert!(replayed.replayed);
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a replayed publication: {made:#?}");

    let metadata = views(&database);
    let change = TagChange { tag: "latest", target: Some("1.0.0"), principal: Some("bob") };
    metadata
        .change_tag(&npm("core"), change)
        .await
        .unwrap();
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a dist-tag move: {made:#?}");

    let change = NoteChange { text: Some("edited\n".to_string()), principal: "bob".to_string() };
    assert!(
        database
            .set_version_note(&npm("core"), "", "2.1.0", change)
            .await
            .unwrap(),
    );
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a note edit: {made:#?}");

    assert!(
        metadata
            .read_view(&npm("core"))
            .await
            .unwrap()
            .is_some(),
    );
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a view read that renders every version: {made:#?}");
    assert!(
        metadata
            .read_view(&npm("core"))
            .await
            .unwrap()
            .is_some(),
    );
    let made = round_trips(&server);
    assert_eq!(made.len(), 1, "a view read the cache answers: {made:#?}");

    assert!(
        metadata
            .remove_package(&npm("core"))
            .await
            .unwrap()
            .is_some(),
    );
    let made = round_trips(&server);
    assert_eq!(made.len(), 2, "a removal: {made:#?}");
}
