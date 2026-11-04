use super::serve_primary;
use crate::LibsqlConnector;
use libsql::{Builder, Connection, Database};
use std::path::Path;

/// A replica at `path`, bootstrapped from the primary's export.
async fn replica(path: &Path, url: &str) -> Database {
    let db = Builder::new_remote_replica(path, url.to_string(), String::new())
        .connector(LibsqlConnector::for_url(url, false).unwrap())
        .read_your_writes(false)
        .build()
        .await
        .unwrap();
    db.sync().await.unwrap();
    db
}

async fn count(conn: &Connection) -> i64 {
    let mut rows = conn
        .query("SELECT count(*) FROM items", ())
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    row.get(0).unwrap()
}

/// Another replica's bootstrap must not take frames from a replica that has
/// not pulled them yet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replica_pulls_a_write_that_another_replica_bootstrap_followed() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let url = format!("http://{address}");
    primary.conn
        .execute("CREATE TABLE items (id INTEGER)", ())
        .await
        .unwrap();
    let first = replica(&directory.path().join("first.db"), &url).await;
    primary.conn
        .execute("INSERT INTO items VALUES (1)", ())
        .await
        .unwrap();
    let second = replica(&directory.path().join("second.db"), &url).await;

    first.sync().await.unwrap();
    let first_count = count(&first.connect().unwrap()).await;
    let second_count = count(&second.connect().unwrap()).await;
    server.abort();
    assert_eq!((first_count, second_count), (1, 1));
}
