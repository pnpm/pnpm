use super::{Adapter, Db};
use crate::metadata::sql::{SqlMetadata, adapter::ReadSource};
use std::{sync::Arc, time::Duration};

async fn schema_waits_longer_than_the_request_timeout(mut db: Db) {
    let Adapter::Network(settings) = &mut db.adapter else { unreachable!() };
    settings.timeout = Duration::from_millis(100);
    settings.startup_timeout = Duration::from_secs(5);
    let holder = db.adapter();
    let metadata = Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "test".into()));
    let (locked, acquired) = tokio::sync::oneshot::channel();
    let holding = tokio::task::spawn_blocking(move || {
        let mut locked = Some(locked);
        holder.write(&mut |connection| {
            connection.lock_schema()?;
            locked.take().unwrap().send(()).unwrap();
            std::thread::sleep(Duration::from_millis(500));
            Ok(())
        })
    });
    acquired.await.unwrap();
    tokio::task::spawn_blocking(move || metadata.ensure_schema()).await.unwrap().unwrap();
    holding.await.unwrap().unwrap();
    assert_request_timeout(&db).await;
}

async fn assert_request_timeout(db: &Db) {
    let adapter = db.adapter();
    let sleep = db.sql("", "SELECT pg_sleep(1)", "SELECT 1 WHERE SLEEP(1) = 0");
    let error = tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            connection.query(sleep, &[]).map(|_| ())
        })
    })
    .await
    .unwrap()
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("timed out")
            || error.contains("statement timeout")
            || error.contains("execution time"),
        "{error}",
    );
}

#[cfg(feature = "backend-postgres")]
mod on_postgres {
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn schema_uses_startup_timeout() {
        super::schema_waits_longer_than_the_request_timeout(
            super::Db::network("PNPR_TEST_POSTGRES_URL").await,
        )
        .await;
    }
}

#[cfg(feature = "backend-mysql")]
mod on_mysql {
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_MYSQL_URL"]
    async fn schema_uses_startup_timeout() {
        super::schema_waits_longer_than_the_request_timeout(
            super::Db::network("PNPR_TEST_MYSQL_URL").await,
        )
        .await;
    }
}
