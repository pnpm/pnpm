//! Binding a metadata database to its hosted store's `store_id`.

use super::{Connection, Freshness, SqlMetadata};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

/// How a metadata database is bound to a hosted store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreBinding {
    /// Bound to this `store_id` in `hosted_store`.
    Bound(String),
    /// No row in `hosted_store`. `has_data` says whether any hosted
    /// catalog, package, blob, staged archive, or removed package row exists.
    Unmarked { has_data: bool },
}

impl SqlMetadata {
    /// How this database is bound to its hosted store.
    pub(crate) async fn store_binding(self: &Arc<Self>) -> Result<StoreBinding> {
        self.read(String::new(), Freshness::Current, move |connection, _| {
            read_store_binding(connection)
        })
        .await
    }

    /// Bind this database to `candidate` when `hosted_store` holds no id yet,
    /// returning the bound `store_id`.
    pub(crate) async fn claim_store_id(self: &Arc<Self>, candidate: String) -> Result<String> {
        self.write(move |connection| claim_store_id(connection, &candidate)).await
    }

    /// Synchronous [`Self::claim_store_id`] for test fixture setup.
    pub(crate) fn claim_store_id_sync(&self, candidate: &str) -> Result<String> {
        self.run_write(|connection| claim_store_id(connection, candidate))
    }
}

fn read_store_binding(connection: &dyn Connection) -> Result<StoreBinding> {
    if let Some(store_id) = read_store_id(connection)? {
        return Ok(StoreBinding::Bound(store_id));
    }
    let has_data = connection
        .query_optional(
            "SELECT 1 FROM catalogs
             UNION ALL SELECT 1 FROM packages
             UNION ALL SELECT 1 FROM blobs
             UNION ALL SELECT 1 FROM staged_archives
             UNION ALL SELECT 1 FROM removed_packages
             LIMIT 1",
            params![],
        )?
        .is_some();
    Ok(StoreBinding::Unmarked { has_data })
}

fn read_store_id(connection: &dyn Connection) -> Result<Option<String>> {
    let Some(mut row) = connection.query_optional(
        "SELECT store_id FROM hosted_store WHERE singleton = 1",
        params![],
    )?
    else {
        return Ok(None);
    };
    let store_id: String = row.get(0)?;
    if store_id.is_empty() {
        return Err(RegistryError::StorageUnavailable {
            reason: "the metadata database holds an empty store_id in hosted_store".to_string(),
        });
    }
    Ok(Some(store_id))
}

fn claim_store_id(connection: &dyn Connection, candidate: &str) -> Result<String> {
    if let Some(existing) = read_store_id(connection)? {
        return Ok(existing);
    }
    connection.execute(
        "INSERT INTO hosted_store (singleton, store_id) VALUES (1, ?1)
         ON CONFLICT DO NOTHING",
        params![candidate],
    )?;
    read_store_id(connection)?
        .ok_or_else(|| RegistryError::Internal {
            reason: "hosted_store holds no store_id after insert".to_string(),
        })
}
