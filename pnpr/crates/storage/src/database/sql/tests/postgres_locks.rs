//! `PostgreSQL` locks end with their transaction, so they hold behind a
//! pooler that gives each transaction another server connection.

use super::{Adapter as Backend, Db};
use crate::database::sql::{
    adapter::{Adapter, WriteTransaction},
    schema,
};
use pnpr_error::Result;
use std::time::Duration;

mod on_postgres {
    use super::{
        Duration, adapters, initialize, lock_schema_in_a_transaction,
        migrate_after_an_ended_migration, migrate_during_an_open_write, write_after_an_ended_write,
    };
    use crate::database::sql::schema;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_session_whose_write_ended_holds_no_write_lock() {
        let (_db, first, second) = adapters().await;
        tokio::task::spawn_blocking(move || write_after_an_ended_write(&*first, &*second))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_session_whose_migration_ended_holds_no_schema_lock() {
        let (_db, first, second) = adapters().await;
        tokio::task::spawn_blocking(move || migrate_after_an_ended_migration(&*first, &*second))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_migration_waits_for_an_open_write() {
        let (_db, first, second) = adapters().await;
        tokio::task::spawn_blocking(move || migrate_during_an_open_write(&*first, &*second))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_transaction_that_is_open_refuses_the_schema_lock() {
        let (_db, first, _second) = adapters().await;
        tokio::task::spawn_blocking(move || lock_schema_in_a_transaction(&*first))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn a_migration_that_waited_reads_what_the_holder_committed() {
        let (_db, first, second) = adapters().await;
        let (locked, acquired) = std::sync::mpsc::channel();
        let holding = tokio::task::spawn_blocking(move || {
            first.write(&mut |connection| {
                connection.lock_schema()?;
                locked.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
                assert!(schema::prepare(connection)?.is_none());
                Ok(())
            })
        });
        tokio::task::spawn_blocking(move || {
            acquired.recv().unwrap();
            initialize(&*second)
        })
        .await
        .unwrap()
        .unwrap();
        holding.await.unwrap().unwrap();
    }
}

/// Two instances on one new database, with a timeout short enough that
/// waiting for a lock the other left behind fails the test.
async fn adapters() -> (Db, Box<dyn Adapter>, Box<dyn Adapter>) {
    let mut db = Db::network("PNPR_TEST_POSTGRES_URL").await;
    let Backend::Network(settings) = &mut db.adapter else { unreachable!() };
    settings.timeout = Duration::from_secs(2);
    let adapters = (db.adapter(), db.adapter());
    (db, adapters.0, adapters.1)
}

fn initialize(adapter: &dyn Adapter) -> Result<()> {
    assert!(schema::prepare_adapter_with_steps(adapter, schema::STEPS)?.is_none());
    Ok(())
}

/// Write through `second` while the session of `first` stays open after its
/// write transaction ended, as a server connection does behind a pooler.
fn write_after_an_ended_write(first: &dyn Adapter, second: &dyn Adapter) -> Result<()> {
    initialize(first)?;
    first.write(&mut |connection| {
        connection.begin_write()?;
        connection.execute_batch("ROLLBACK")?;
        second.write(&mut |other| WriteTransaction::begin(other)?.commit())
    })
}

/// Migrate through `second` while the session of `first` stays open after its
/// migration transaction ended.
fn migrate_after_an_ended_migration(first: &dyn Adapter, second: &dyn Adapter) -> Result<()> {
    first.write(&mut |connection| {
        connection.lock_schema()?;
        connection.execute_batch("ROLLBACK")?;
        initialize(second)
    })
}

/// Begin a migration's write through `second` while `first` holds the write
/// lock. It waits until its statement times out.
fn migrate_during_an_open_write(first: &dyn Adapter, second: &dyn Adapter) -> Result<()> {
    initialize(first)?;
    first.write(&mut |connection| {
        let held = WriteTransaction::begin(connection)?;
        let waiting = second.write(&mut |other| {
            other.lock_schema()?;
            other.begin_write()
        });
        let error = waiting.unwrap_err().to_string();
        // The server and the client hold the same deadline.
        assert!(
            error.contains("statement timeout") || error.contains("database operation timed out"),
            "{error}",
        );
        held.commit()
    })
}

fn lock_schema_in_a_transaction(adapter: &dyn Adapter) -> Result<()> {
    initialize(adapter)?;
    adapter.write(&mut |connection| {
        connection.begin_read()?;
        let error = connection
            .lock_schema()
            .unwrap_err()
            .to_string();
        assert!(error.contains("schema lock requested inside a transaction"), "{error}");
        Ok(())
    })
}
