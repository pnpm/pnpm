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
    let initializing = Arc::clone(&metadata);
    tokio::task::spawn_blocking(move || initializing.ensure_schema()).await.unwrap().unwrap();
    holding.await.unwrap().unwrap();
    assert_request_timeout(
        metadata,
        db.sql("", "SELECT pg_sleep(1)", "SELECT 1 WHERE SLEEP(1) = 0"),
    )
    .await;
}

async fn initialization_reuses_pool_capacity(mut db: Db) {
    let Adapter::Network(settings) = &mut db.adapter else { unreachable!() };
    settings.max_connections = Some(1);
    settings.timeout = Duration::from_millis(250);
    settings.startup_timeout = Duration::from_secs(5);
    let metadata = Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "test".into()));
    let holder = Arc::clone(&metadata);
    let identity = db.sql("", "SELECT pg_backend_pid()", "SELECT CAST(CONNECTION_ID() AS SIGNED)");
    let (locked, acquired) = tokio::sync::oneshot::channel();
    let holding = tokio::task::spawn_blocking(move || {
        let mut locked = Some(locked);
        holder.adapter.write(&mut |connection| {
            let id = connection.query(identity, &[])?[0].get::<i64>(0)?;
            locked.take().unwrap().send(id).unwrap();
            std::thread::sleep(Duration::from_millis(750));
            Ok(())
        })
    });
    let expected = acquired.await.unwrap();
    let initializing = Arc::clone(&metadata);
    tokio::task::spawn_blocking(move || {
        initializing.adapter.initialize(&mut |connection| {
            assert_eq!(connection.query(identity, &[])?[0].get::<i64>(0)?, expected);
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();
    holding.await.unwrap().unwrap();
    let sleep = db.sql("", "SELECT pg_sleep(1)", "SELECT 1 WHERE SLEEP(1) = 0");
    assert_request_timeout(Arc::clone(&metadata), sleep).await;
    let failing = Arc::clone(&metadata);
    let error = tokio::task::spawn_blocking(move || {
        failing.adapter.initialize(&mut |_connection| {
            Err(pnpr_error::RegistryError::Internal { reason: "initialization failed".into() })
        })
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.to_string().contains("initialization failed"), "{error}");
    assert_request_timeout(metadata, sleep).await;
}

async fn slow_connection_uses_startup_deadline(mut db: Db) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = listener.local_addr().unwrap();
    let Adapter::Network(settings) = &mut db.adapter else { unreachable!() };
    let original_url = settings.url.clone();
    let (credentials, endpoint) = settings.url.rsplit_once('@').unwrap();
    let (server_address, database) = endpoint.split_once('/').unwrap();
    let server_address = server_address.to_string();
    settings.url = format!("{credentials}@{proxy_address}/{database}");
    settings.timeout = Duration::from_millis(100);
    settings.startup_timeout = Duration::from_secs(5);
    let proxy = tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut server = tokio::net::TcpStream::connect(server_address).await.unwrap();
        tokio::io::copy_bidirectional(&mut client, &mut server).await
    });
    let metadata = SqlMetadata::with_adapter(db.instance_adapter(), "test".into());
    let Adapter::Network(settings) = &mut db.adapter else { unreachable!() };
    settings.url = original_url;
    tokio::task::spawn_blocking(move || metadata.ensure_schema()).await.unwrap().unwrap();
    proxy.await.unwrap().unwrap();
}

async fn assert_request_timeout(metadata: Arc<SqlMetadata>, sleep: &'static str) {
    let error = tokio::task::spawn_blocking(move || {
        metadata.adapter.read(ReadSource::Primary, &mut |connection| {
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
    async fn slow_connection_opening_uses_startup_timeout() {
        super::slow_connection_uses_startup_deadline(
            super::Db::network("PNPR_TEST_POSTGRES_URL").await,
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_pending_lock_query_does_not_add_a_cleanup_timeout() {
        use super::{Adapter, Duration};
        let mut db = super::Db::network("PNPR_TEST_POSTGRES_URL").await;
        let Adapter::Network(settings) = &mut db.adapter else { unreachable!() };
        settings.startup_timeout = Duration::from_millis(500);
        let holder = db.adapter();
        let initializer = db.adapter();
        let (locked, acquired) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let holding = tokio::task::spawn_blocking(move || {
            let mut locked = Some(locked);
            holder.write(&mut |connection| {
                connection.lock_schema()?;
                locked.take().unwrap().send(()).unwrap();
                released
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Ok(())
            })
        });
        acquired.await.unwrap();
        let initializing = tokio::task::spawn_blocking(move || {
            initializer.initialize(&mut |connection| {
                // Model a lock query that remains pending after its client deadline.
                connection.execute_batch("SET statement_timeout = 0")?;
                connection.lock_schema()
            })
        });
        let result = tokio::time::timeout(Duration::from_millis(900), initializing).await;
        release.send(()).unwrap();
        holding.await.unwrap().unwrap();
        let error = result
            .expect("cleanup waited for another deadline")
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn initialization_shares_pool_capacity_and_preserves_request_timeout() {
        super::initialization_reuses_pool_capacity(
            super::Db::network("PNPR_TEST_POSTGRES_URL").await,
        )
        .await;
    }

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
    async fn slow_connection_opening_uses_startup_timeout() {
        super::slow_connection_uses_startup_deadline(
            super::Db::network("PNPR_TEST_MYSQL_URL").await,
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_MYSQL_URL"]
    async fn initialization_shares_pool_capacity_and_preserves_request_timeout() {
        super::initialization_reuses_pool_capacity(super::Db::network("PNPR_TEST_MYSQL_URL").await)
            .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_MYSQL_URL"]
    async fn schema_uses_startup_timeout() {
        super::schema_waits_longer_than_the_request_timeout(
            super::Db::network("PNPR_TEST_MYSQL_URL").await,
        )
        .await;
    }
}
