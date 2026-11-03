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
        let settings = LibsqlSettings {
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
        };
        Arc::new(SqlMetadata::open_libsql(&settings))
    }

    fn statements(&self) -> usize {
        self.primary.pipelines.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Set to a libSQL URL, such as a throwaway Turso database's, the conformance
/// tests run on that database instead of the local primary. Every test drops
/// all of its tables first, so never name a database whose data you keep.
/// The tests take turns on it, so run them with `--test-threads 1`; nextest
/// would otherwise time out the ones left waiting.
const EXTERNAL_URL: &str = "PNPR_TEST_LIBSQL_URL";

/// The auth token for [`EXTERNAL_URL`].
const EXTERNAL_AUTH_TOKEN: &str = "PNPR_TEST_LIBSQL_AUTH_TOKEN";

/// Locked by the test that has the external database, so the tests take
/// turns on it even from the separate processes nextest runs them in.
fn external_turn_file() -> std::path::PathBuf {
    std::env::temp_dir().join("pnpr-test-libsql.lock")
}

/// The primary a conformance test runs on: a local one of its own, or the
/// database [`EXTERNAL_URL`] names, emptied for this test and held until it
/// ends.
pub(super) enum ConformancePrimary {
    Local(Server),
    External {
        settings: LibsqlSettings,
        /// Where the instances keep their replicas.
        dir: TempDir,
        /// Holds the lock of [`external_turn_file`] until the test ends.
        _turn: std::fs::File,
    },
}

pub(super) async fn conformance_primary() -> ConformancePrimary {
    let Ok(url) = std::env::var(EXTERNAL_URL) else {
        return ConformancePrimary::Local(server().await);
    };
    let turn = tokio::task::spawn_blocking(|| {
        let file = std::fs::File::create(external_turn_file()).unwrap();
        file.lock().unwrap();
        file
    })
    .await
    .unwrap();
    let settings = LibsqlSettings {
        url,
        auth_token: std::env::var(EXTERNAL_AUTH_TOKEN).ok(),
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    };
    drop_tables(&settings).await;
    ConformancePrimary::External { settings, dir: TempDir::new().unwrap(), _turn: turn }
}

/// Drop every table of the database `settings` name, the newest first, so a
/// table goes before the tables it references.
async fn drop_tables(settings: &LibsqlSettings) {
    let connector = pnpr_libsql::connector(settings, EXTERNAL_URL).unwrap();
    let database = pnpr_libsql::open_primary(settings, connector).await.unwrap();
    let connection = database.connect().unwrap();
    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY rowid DESC",
            (),
        )
        .await
        .unwrap();
    let mut tables = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        tables.push(row.get::<String>(0).unwrap());
    }
    for table in tables {
        let table = table.replace('"', r#""""#);
        connection
            .execute(&format!(r#"DROP TABLE "{table}""#), ())
            .await
            .unwrap();
    }
}

impl ConformancePrimary {
    /// The metadata of one pnpr instance on this primary, as
    /// [`Server::instance`] opens it.
    pub(super) fn instance(
        &self,
        replica: Option<&str>,
        sync_interval_secs: u64,
    ) -> Arc<SqlMetadata> {
        match self {
            Self::Local(server) => server.instance(replica, sync_interval_secs),
            Self::External { settings, dir, .. } => {
                Arc::new(SqlMetadata::open_libsql(&LibsqlSettings {
                    replica_path: replica.map(|name| dir.path().join(name).join("replica.db")),
                    sync_interval_secs: Some(sync_interval_secs),
                    ..settings.clone()
                }))
            }
        }
    }
}

fn views(database: &Arc<SqlMetadata>) -> Arc<dyn PackageMetadata> {
    Arc::new(database.view("", Ecosystem::Npm))
}

async fn publish(metadata: &dyn PackageMetadata, versions: &[&str]) {
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

async fn latest(metadata: &dyn PackageMetadata) -> Option<String> {
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
    publish(&*views(&first), &["1.0.0"]).await;
    assert_eq!(latest(&*views(&second)).await.as_deref(), Some("1.0.0"));
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
    publish(&*writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 3600));
    assert_eq!(latest(&*reader).await.as_deref(), Some("1.0.0"), "bootstrapped from the primary");
    assert!(replica_exists(&server, "reader"));

    let before = server.statements();
    assert_eq!(latest(&*reader).await.as_deref(), Some("1.0.0"));
    assert_eq!(server.statements(), before, "a view is read from the replica");

    publish(&*writer, &["1.0.0", "2.0.0"]).await;
    assert_eq!(
        latest(&*reader).await.as_deref(),
        Some("1.0.0"),
        "another instance's commit reaches the replica at its next sync",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_that_must_be_current_does_not_take_the_replica_copy() {
    let server = server().await;
    let writer = views(&server.instance(None, 60));
    publish(&*writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 3600));
    assert!(latest(&*reader).await.is_some());
    publish(&*writer, &["1.0.0", "2.0.0"]).await;

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
    assert_eq!(view.generation.map(|generation| generation.value), Some(2));
    assert_eq!(
        latest(&*reader).await.as_deref(),
        Some("2.0.0"),
        "the replica synced past the generation the read asked for",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_instance_reads_its_own_commits_from_its_replica() {
    let server = server().await;
    let database = server.instance(Some("own"), 3600);
    let metadata = views(&database);
    publish(&*metadata, &["1.0.0"]).await;
    publish(&*metadata, &["1.0.0", "2.0.0"]).await;
    assert_eq!(latest(&*metadata).await.as_deref(), Some("2.0.0"));
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
    publish(&*writer, &["1.0.0"]).await;
    let reader = views(&server.instance(Some("reader"), 1));
    assert!(latest(&*reader).await.is_some());
    publish(&*writer, &["1.0.0", "2.0.0"]).await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(latest(&*reader).await.as_deref(), Some("2.0.0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_that_overtakes_a_sync_leaves_the_replica_stale() {
    use std::sync::atomic::Ordering;
    let server = server().await;
    let metadata = views(&server.instance(Some("own"), 3600));
    // The write leaves the replica stale, so the next view syncs it.
    publish(&*metadata, &["1.0.0"]).await;
    server.primary.sync_stall.enabled.store(true, Ordering::SeqCst);
    let reader = Arc::clone(&metadata);
    let read = tokio::spawn(async move { latest(&*reader).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.primary.sync_stall.started.notified(),
    )
    .await
    .expect("the view's sync reaches the primary");
    // Committed while the view's pull answers with the primary as it was.
    publish(&*metadata, &["1.0.0", "2.0.0"]).await;
    read.await.unwrap();
    server.primary.sync_stall.enabled.store(false, Ordering::SeqCst);
    assert_eq!(latest(&*metadata).await.as_deref(), Some("2.0.0"));
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
    assert!(Arc::ptr_eq(first.sql_metadata().unwrap(), second.sql_metadata().unwrap()));
}
