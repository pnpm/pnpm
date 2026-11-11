//! Binding a metadata database to its hosted store's `store_id`.

use super::{Connection, Freshness, SqlMetadata};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

const PENDING_PREFIX: &str = "pending:";

/// How a metadata database is bound to a hosted store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreBinding {
    /// Confirmed binding to this `store_id` in `hosted_store`.
    Bound(String),
    /// An in-progress binding with this database nonce in `hosted_store`
    /// whose `layout.json` write has not yet been confirmed.
    Pending(String),
    /// No row in `hosted_store`. `has_data` says whether any hosted
    /// namespace, package, blob, staged archive, or removed package row exists.
    Unmarked { has_data: bool },
}

/// Outcome of starting a pending store claim in `hosted_store`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreClaim {
    /// `hosted_store` holds `pending:<nonce>` for an in-progress initialization.
    Pending(String),
    /// `hosted_store` is already confirmed bound to `<store_id>`.
    Bound(String),
}

impl SqlMetadata {
    /// How this database is bound to its hosted store.
    pub(crate) async fn store_binding(self: &Arc<Self>) -> Result<StoreBinding> {
        self.read(String::new(), Freshness::Current, move |connection, _| {
            read_store_binding(connection)
        })
        .await
    }

    /// Record `pending:<candidate_nonce>` in `hosted_store` when it holds no
    /// row yet, returning the active pending nonce or confirmed `store_id`.
    pub(crate) async fn begin_store_claim(
        self: &Arc<Self>,
        candidate_nonce: String,
    ) -> Result<StoreClaim> {
        self.write(move |connection| begin_store_claim(connection, &candidate_nonce)).await
    }

    /// Confirm `pending:<pending_nonce>` in `hosted_store` as `<store_id>` once
    /// `layout.json` holds `<store_id>`.
    pub(crate) async fn confirm_store_claim(
        self: &Arc<Self>,
        pending_nonce: String,
        store_id: String,
    ) -> Result<StoreBinding> {
        self.write(move |connection| confirm_store_claim(connection, &pending_nonce, &store_id))
            .await
    }

    /// Remove `pending:<pending_nonce>` when the hosted store is already bound
    /// to another database.
    pub(crate) async fn clear_pending_store_claim(
        self: &Arc<Self>,
        pending_nonce: String,
    ) -> Result<()> {
        self.write(move |connection| clear_pending_store_claim(connection, &pending_nonce)).await
    }

    /// Synchronous confirmed claim for test fixture setup.
    pub(crate) fn claim_store_id_sync(&self, candidate: &str) -> Result<String> {
        self.run_write(|connection| claim_store_id(connection, candidate))
    }
}

fn read_store_binding(connection: &dyn Connection) -> Result<StoreBinding> {
    if let Some(raw_id) = read_raw_store_id(connection)? {
        if let Some(pending_id) = raw_id.strip_prefix(PENDING_PREFIX) {
            if pending_id.is_empty() {
                return Err(empty_store_id_error());
            }
            return Ok(StoreBinding::Pending(pending_id.to_string()));
        }
        return Ok(StoreBinding::Bound(raw_id));
    }
    let has_data = connection
        .query_optional(
            "SELECT 1 FROM namespaces
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

fn read_raw_store_id(connection: &dyn Connection) -> Result<Option<String>> {
    let Some(mut row) = connection.query_optional(
        "SELECT store_id FROM hosted_store WHERE singleton = 1",
        params![],
    )?
    else {
        return Ok(None);
    };
    let store_id: String = row.get(0)?;
    if store_id.is_empty() {
        return Err(empty_store_id_error());
    }
    Ok(Some(store_id))
}

fn begin_store_claim(connection: &dyn Connection, candidate_nonce: &str) -> Result<StoreClaim> {
    match read_store_binding(connection)? {
        StoreBinding::Bound(id) => return Ok(StoreClaim::Bound(id)),
        StoreBinding::Pending(nonce) => return Ok(StoreClaim::Pending(nonce)),
        StoreBinding::Unmarked { has_data: true } => {
            return Err(RegistryError::StorageUnavailable {
                reason: "the metadata database has no store_id in hosted_store and holds data from an earlier schema".to_string(),
            });
        }
        StoreBinding::Unmarked { has_data: false } => {}
    }
    let pending = format!("{PENDING_PREFIX}{candidate_nonce}");
    connection.execute(
        "INSERT INTO hosted_store (singleton, store_id) VALUES (1, ?1)
         ON CONFLICT DO NOTHING",
        params![pending.as_str()],
    )?;
    match read_store_binding(connection)? {
        StoreBinding::Bound(id) => Ok(StoreClaim::Bound(id)),
        StoreBinding::Pending(nonce) => Ok(StoreClaim::Pending(nonce)),
        StoreBinding::Unmarked { .. } => Err(RegistryError::Internal {
            reason: "hosted_store holds no store_id after claim".to_string(),
        }),
    }
}

fn confirm_store_claim(
    connection: &dyn Connection,
    pending_nonce: &str,
    store_id: &str,
) -> Result<StoreBinding> {
    let pending = format!("{PENDING_PREFIX}{pending_nonce}");
    connection.execute(
        "UPDATE hosted_store SET store_id = ?1 WHERE singleton = 1 AND store_id = ?2",
        params![store_id, pending.as_str()],
    )?;
    read_store_binding(connection)
}

fn clear_pending_store_claim(connection: &dyn Connection, pending_nonce: &str) -> Result<()> {
    let pending = format!("{PENDING_PREFIX}{pending_nonce}");
    connection.execute(
        "DELETE FROM hosted_store WHERE singleton = 1 AND store_id = ?1",
        params![pending.as_str()],
    )?;
    Ok(())
}

fn claim_store_id(connection: &dyn Connection, candidate: &str) -> Result<String> {
    match read_store_binding(connection)? {
        StoreBinding::Bound(existing) => return Ok(existing),
        StoreBinding::Pending(_) => {
            connection.execute(
                "UPDATE hosted_store SET store_id = ?1 WHERE singleton = 1",
                params![candidate],
            )?;
            return Ok(candidate.to_string());
        }
        StoreBinding::Unmarked { .. } => {}
    }
    connection.execute(
        "INSERT INTO hosted_store (singleton, store_id) VALUES (1, ?1)
         ON CONFLICT DO NOTHING",
        params![candidate],
    )?;
    match read_store_binding(connection)? {
        StoreBinding::Bound(id) | StoreBinding::Pending(id) => Ok(id),
        StoreBinding::Unmarked { .. } => Err(RegistryError::Internal {
            reason: "hosted_store holds no store_id after insert".to_string(),
        }),
    }
}

fn empty_store_id_error() -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: "the metadata database holds an empty store_id in hosted_store".to_string(),
    }
}
