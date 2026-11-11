//! Durable, coalesced CDN invalidations committed with the data they invalidate.

use super::{SqlMetadata, adapter::Connection, batch::WriteTransaction, transactions::Freshness};
use crate::Storage;
use pnpr_error::Result;
use std::sync::Arc;

const SUBSCRIPTION: &str = "cdn-subscription";
const PENDING: &str = "cdn-pending";

/// A package name, or `*` for every cached response, and its delivery generation.
#[derive(Debug, Clone)]
pub struct CdnInvalidation {
    pub package: String,
    pub generation: i64,
}

/// Subscription persists so writes by other processes also record invalidations.
pub(super) fn enqueue(connection: &dyn Connection, package: &str) -> Result<()> {
    connection.execute(
        "UPDATE record_generation SET generation = generation + 1 WHERE singleton = 1
         AND EXISTS (SELECT 1 FROM registry_records WHERE kind = ?1 AND name = 'default')",
        params![SUBSCRIPTION],
    )?;
    connection.execute(
        "INSERT INTO registry_records (kind, name, generation, body)
         SELECT ?1, ?2, generation, ?3 FROM record_generation WHERE singleton = 1
         AND EXISTS (SELECT 1 FROM registry_records WHERE kind = ?4 AND name = 'default')
         ON CONFLICT (kind, name) DO UPDATE SET generation = excluded.generation",
        params![PENDING, package, &[] as &[u8], SUBSCRIPTION],
    )?;
    Ok(())
}

impl Storage {
    /// Enable transactional invalidation recording and invalidate pre-startup cache entries.
    pub async fn enable_cdn_invalidations(&self, target: &str) -> Result<()> {
        let target = target.as_bytes().to_vec();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            transaction.execute(
                "INSERT INTO registry_records (kind, name, generation, body)
                 VALUES (?1, 'default', 0, ?2) ON CONFLICT DO NOTHING",
                params![SUBSCRIPTION, &target],
            )?;
            let mut registered = transaction.query_optional(
                "SELECT body FROM registry_records WHERE kind = ?1 AND name = 'default'",
                params![SUBSCRIPTION],
            )?.ok_or_else(|| pnpr_error::RegistryError::Internal { reason: "CDN subscription is missing".to_string() })?;
            if registered.get::<Vec<u8>>(0)? != target {
                return Err(pnpr_error::RegistryError::InvalidConfig {
                    reason: "all servers sharing a database must use the same cloudflareCdn zoneId and cacheTag".to_string(),
                });
            }
            enqueue(&*transaction, "*")?;
            transaction.commit()
        })
        .await
    }

    /// Read pending policy changes and the oldest package changes from the primary.
    pub async fn pending_cdn_invalidations(&self) -> Result<Vec<CdnInvalidation>> {
        self.database.pending_cdn_invalidations().await
    }

    /// Acknowledge only the delivered generations, retaining concurrent changes.
    pub async fn acknowledge_cdn_invalidations(
        &self,
        delivered: Vec<CdnInvalidation>,
    ) -> Result<()> {
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            for event in &delivered {
                transaction.execute(
                    "DELETE FROM registry_records WHERE kind = ?1 AND name = ?2 AND generation = ?3",
                    params![PENDING, &event.package, event.generation],
                )?;
            }
            transaction.commit()
        }).await
    }
}

impl SqlMetadata {
    async fn pending_cdn_invalidations(self: &Arc<Self>) -> Result<Vec<CdnInvalidation>> {
        self.read(String::new(), Freshness::Current, |connection, _| {
            // Reserve one row for policy invalidation within the 30-tag purge limit.
            let rows = connection.query(
                "SELECT name, generation FROM (
                   SELECT name, generation, 0 AS priority FROM registry_records WHERE kind = ?1 AND name = '*'
                   UNION ALL SELECT name, generation, 1 AS priority FROM (
                     SELECT name, generation FROM registry_records
                     WHERE kind = ?1 AND name <> '*' ORDER BY generation LIMIT 29
                   ) AS packages
                 ) AS pending ORDER BY priority, generation",
                params![PENDING],
            )?;
            rows.into_iter()
                .map(|mut row| {
                    Ok(CdnInvalidation { package: row.get(0)?, generation: row.get(1)? })
                })
                .collect()
        })
        .await
    }
}
