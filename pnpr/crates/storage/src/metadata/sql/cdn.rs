//! Durable, coalesced CDN invalidations committed with the data they invalidate.

use super::{SqlMetadata, adapter::Connection, batch::WriteTransaction, transactions::Freshness};
use crate::Storage;
use pnpr_error::{RegistryError, Result};
use pnpr_sql::DialectSql;
use std::sync::Arc;

/// A package name, or `*` for every cached response, and its delivery generation.
#[derive(Debug, Clone)]
pub struct CdnInvalidation {
    pub package: String,
    pub generation: i64,
}

const ENQUEUE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO cdn_purge_queue (package, generation)
     SELECT ?1, generation FROM record_generation WHERE singleton = 1
     AND EXISTS (SELECT 1 FROM cdn_subscriptions WHERE singleton = 1)
     ON CONFLICT (package) DO UPDATE SET generation = excluded.generation",
    "INSERT INTO cdn_purge_queue (package, generation)
     SELECT ?1, generation FROM record_generation WHERE singleton = 1
     AND EXISTS (SELECT 1 FROM cdn_subscriptions WHERE singleton = 1)
     ON DUPLICATE KEY UPDATE generation = VALUES(generation)",
);

/// Subscription persists so writes by other processes also record invalidations.
pub(super) fn enqueue(connection: &dyn Connection, package: &str) -> Result<()> {
    connection.execute(
        "UPDATE record_generation SET generation = generation + 1 WHERE singleton = 1
         AND EXISTS (SELECT 1 FROM cdn_subscriptions WHERE singleton = 1)",
        params![],
    )?;
    connection.execute(ENQUEUE.get(connection.dialect()), params![package])?;
    Ok(())
}

const SUBSCRIBE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO cdn_subscriptions (singleton, target)
     VALUES (1, ?1) ON CONFLICT DO NOTHING",
    "INSERT INTO cdn_subscriptions (singleton, target)
     VALUES (1, ?1) ON DUPLICATE KEY UPDATE singleton = cdn_subscriptions.singleton",
);

impl Storage {
    /// Enable transactional invalidation recording and invalidate pre-startup cache entries.
    pub async fn enable_cdn_invalidations(&self, target: &str) -> Result<()> {
        let target = target.to_string();
        self.database
            .write(move |connection| {
                let transaction = WriteTransaction::begin(connection)?;
                transaction.execute(
                    SUBSCRIBE.get(transaction.dialect()),
                    params![&target],
                )?;
                let mut registered = transaction
                    .query_optional(
                        "SELECT target FROM cdn_subscriptions WHERE singleton = 1",
                        params![],
                    )?
                    .ok_or_else(|| RegistryError::Internal {
                        reason: "CDN subscription is missing".to_string(),
                    })?;
                if registered.get::<String>(0)? != target {
                    return Err(RegistryError::InvalidConfig {
                        reason: "all servers sharing a database must use the same cloudflareCdn zoneId and cacheTag"
                            .to_string(),
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
                    "DELETE FROM cdn_purge_queue WHERE package = ?1 AND generation = ?2",
                    params![&event.package, event.generation],
                )?;
            }
            transaction.commit()
        })
        .await
    }
}

impl SqlMetadata {
    async fn pending_cdn_invalidations(self: &Arc<Self>) -> Result<Vec<CdnInvalidation>> {
        self.read(String::new(), Freshness::Current, |connection, _| {
            let rows = connection.query(
                "SELECT package, generation FROM (
                   SELECT package, generation, 0 AS priority FROM cdn_purge_queue WHERE package = '*'
                   UNION ALL SELECT package, generation, 1 AS priority FROM (
                     SELECT package, generation FROM cdn_purge_queue
                     WHERE package <> '*' ORDER BY generation LIMIT 29
                   ) AS packages
                 ) AS pending ORDER BY priority, generation",
                params![],
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
