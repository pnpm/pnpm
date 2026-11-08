use super::{
    super::adapter::{Adapter, Connection, ReadSource},
    Db, config,
};
use crate::{Storage, Store, metadata::SqlMetadata};
use pnpr_error::{RegistryError, Result};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

#[derive(Debug)]
struct LostAcknowledgement {
    inner: Box<dyn Adapter>,
    state: Arc<AtomicU8>,
}

impl Adapter for LostAcknowledgement {
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        match self.state.load(Ordering::Acquire) {
            0 => self.inner.write(operation),
            1 => {
                self.inner.write(operation)?;
                self.state.store(2, Ordering::Release);
                Err(RegistryError::Internal { reason: "commit acknowledgement lost".to_string() })
            }
            _ => Err(RegistryError::Internal { reason: "database unavailable".to_string() }),
        }
    }

    fn read(
        &self,
        source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<()> {
        self.inner.read(source, operation)
    }

    fn is_retryable(&self, error: &RegistryError) -> bool {
        self.inner.is_retryable(error)
    }
}

async fn staged_cleanup_distinguishes_failed_writes_from_lost_acknowledgements(db: Db) {
    let state = Arc::new(AtomicU8::new(0));
    let adapter = LostAcknowledgement { inner: db.adapter(), state: Arc::clone(&state) };
    let database = Arc::new(SqlMetadata::with_adapter(Box::new(adapter), "test database".into()));
    let opening = Arc::clone(&database);
    tokio::task::spawn_blocking(move || opening.ensure_schema()).await.unwrap().unwrap();
    let config = config(&db.tmp);
    let storage = Storage::from_parts(
        Arc::new(Store::new(config.hosted_dir)),
        Store::new(config.cache_dir),
        database,
    );
    let id = "11111111-2222-3333-4444-555555555555";
    storage.create_staged_body(id, b"body").await.unwrap();
    state.store(2, Ordering::Release);
    assert!(storage.create_staged_meta(id, b"metadata").await.is_err());
    assert!(
        storage
            .read_staged_body(id)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        storage
            .read_staged_meta(id)
            .await
            .unwrap()
            .is_none(),
    );

    storage.create_staged_body(id, b"body").await.unwrap();
    state.store(1, Ordering::Release);
    assert!(storage.create_staged_meta(id, b"metadata").await.is_err());
    assert_eq!(
        storage
            .read_staged_meta(id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        b"metadata",
    );
    assert_eq!(
        storage
            .read_staged_body(id)
            .await
            .unwrap()
            .unwrap(),
        b"body",
    );

    state.store(0, Ordering::Release);
    assert!(storage.remove_staged(id).await.unwrap());
    assert!(
        storage
            .read_staged_body(id)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        storage
            .read_staged_meta(id)
            .await
            .unwrap()
            .is_none(),
    );
}

conformance!(staged_cleanup_distinguishes_failed_writes_from_lost_acknowledgements);
