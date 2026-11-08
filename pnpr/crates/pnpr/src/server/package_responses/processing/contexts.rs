use pnpr_storage::{BoundedCache, DerivedTar};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

const CONTEXT_BUDGET: usize = 1024 * 1024;

type Table = BTreeMap<String, DerivedTar>;

#[derive(Clone)]
struct Context {
    // Prevent address reuse without retaining table entries.
    table: Weak<Table>,
    digest: Arc<str>,
}

#[derive(Default)]
pub(super) struct TarContexts {
    entries: Mutex<BoundedCache<usize, Context>>,
}

impl TarContexts {
    pub(super) fn digest(&self, table: &Arc<Table>) -> Result<Arc<str>, pnpr_error::RegistryError> {
        let key = Arc::as_ptr(table).addr();
        let cached = self.entries
            .lock()
            .expect("tar context cache poisoned")
            .get(&key)
            .cloned();
        if let Some(cached) = cached
            && cached.table.upgrade().is_some_and(|snapshot| Arc::ptr_eq(&snapshot, table))
        {
            return Ok(cached.digest);
        }
        let bytes = serde_json::to_vec(table.as_ref())?;
        let digest: Arc<str> = super::super::super::ecosystem::sha256_hex(&bytes).into();
        let context = Context { table: Arc::downgrade(table), digest: Arc::clone(&digest) };
        self.entries
            .lock()
            .expect("tar context cache poisoned")
            .insert(key, context, 256, CONTEXT_BUDGET);
        Ok(digest)
    }
}
