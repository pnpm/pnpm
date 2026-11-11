use super::{
    super::{
        SqlMetadata,
        adapter::{Adapter, Connection, ReadSource, Snapshot},
        transactions::Freshness,
    },
    Db,
};
use pnpr_error::{RegistryError, Result};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Default)]
struct PendingProbe {
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    ready: Condvar,
    calls: AtomicUsize,
}

#[derive(Debug)]
struct StalledPrimary {
    inner: Box<dyn Adapter>,
    probe: Arc<PendingProbe>,
}

impl Adapter for StalledPrimary {
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        self.inner.write(operation)
    }

    fn read(
        &self,
        source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<Snapshot> {
        if matches!(source, ReadSource::Replica) {
            return self.inner.read(source, operation);
        }
        self.probe.calls.fetch_add(1, Ordering::AcqRel);
        self.probe.entered.notify_one();
        let mut released = self.probe.released.lock().unwrap();
        while !*released {
            released = self.probe.ready.wait(released).unwrap();
        }
        Err(RegistryError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
            Some("query canceled".to_string()),
        )))
    }

    fn is_retryable(&self, error: &RegistryError) -> bool {
        self.inner.is_retryable(error)
    }

    fn has_replica(&self) -> bool {
        true
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_or_canceled_probe_does_not_block_request_bursts_or_replica_reads() {
    let db = Db::sqlite();
    let probe = Arc::new(PendingProbe::default());
    let adapter = StalledPrimary { inner: db.adapter(), probe: Arc::clone(&probe) };
    let metadata = Arc::new(SqlMetadata::with_adapter(Box::new(adapter), "test".to_string()));
    metadata.ensure_schema().unwrap();
    metadata.schema_monitor.expire();
    metadata.check_schema().await.unwrap();
    probe.entered.notified().await;

    let result = tokio::time::timeout(Duration::from_secs(1), async {
        for _ in 0..128 {
            metadata.check_schema().await.unwrap();
            metadata.ensure_schema().unwrap();
        }
        metadata.read("test".to_string(), Freshness::Bounded, |_, _| Ok(7)).await.unwrap()
    })
    .await;
    *probe.released.lock().unwrap() = true;
    probe.ready.notify_all();
    assert_eq!(result.expect("a schema probe stalled unrelated requests"), 7);
    assert_eq!(probe.calls.load(Ordering::Acquire), 1);
    tokio::time::timeout(Duration::from_secs(1), async {
        while metadata.schema_monitor.is_checking() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    metadata.schema_monitor.expire();
    metadata.ensure_schema().unwrap();
    assert_eq!(probe.calls.load(Ordering::Acquire), 2);
    metadata.check_schema().await.unwrap();
}
