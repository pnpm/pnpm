use super::{CanonicalPackageName, DocumentWrite, RegistryError, Result, Storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) const NAMESPACE: &str = ".dist-stats";
const CACHE_TTL: Duration = Duration::from_secs(30);
const CACHE_CAPACITY: usize = 4096;
const RECORD_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistStats {
    pub file_count: u64,
    pub unpacked_size: u64,
    pub bins: Option<serde_json::Value>,
}

type Table = Arc<BTreeMap<String, DistStats>>;

#[derive(Debug, Default)]
pub(super) struct DistStatsCache {
    tables: Mutex<HashMap<String, CachedTable>>,
}

#[derive(Debug)]
struct CachedTable {
    read_at: Instant,
    table: Table,
}

impl DistStatsCache {
    fn get(&self, name: &CanonicalPackageName) -> Option<Table> {
        let tables = self.tables.lock().expect("dist stats cache lock poisoned");
        let cached = tables.get(name.as_str())?;
        (cached.read_at.elapsed() < CACHE_TTL).then(|| Arc::clone(&cached.table))
    }

    fn put(&self, name: &CanonicalPackageName, table: Table, read_at: Instant) {
        let mut tables = self.tables.lock().expect("dist stats cache lock poisoned");
        if tables
            .get(name.as_str())
            .is_some_and(|cached| cached.read_at > read_at)
        {
            return;
        }
        if tables.len() >= CACHE_CAPACITY {
            tables.clear();
        }
        tables.insert(name.as_str().to_string(), CachedTable { read_at, table });
    }
}

impl Storage {
    pub async fn read_dist_stats(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Arc<BTreeMap<String, DistStats>>> {
        if let Some(table) = self.dist_stats_cache.get(name) {
            return Ok(table);
        }
        let read_at = Instant::now();
        let document = self.dist_stats.read_document(name).await?;
        let table: Table = Arc::new(
            document
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default(),
        );
        self.dist_stats_cache.put(name, Arc::clone(&table), read_at);
        Ok(table)
    }

    pub async fn record_dist_stats(
        &self,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
        stats: DistStats,
    ) -> Result<()> {
        for _ in 0..RECORD_ATTEMPTS {
            let current = self.dist_stats.read_document_for_update(name).await?;
            let (mut table, version) = match current {
                Some(document) => (
                    serde_json::from_slice::<BTreeMap<String, DistStats>>(&document.bytes)
                        .unwrap_or_default(),
                    Some(document.version),
                ),
                None => (BTreeMap::new(), None),
            };
            if table.get(tgz_integrity) == Some(&stats) {
                return Ok(());
            }
            table.insert(tgz_integrity.to_string(), stats.clone());
            let bytes = serde_json::to_vec(&table)
                .map_err(|err| RegistryError::Internal {
                    reason: format!("encode dist statistics: {err}"),
                })?;
            if self.dist_stats.write_document_if_current(name, &bytes, version.as_ref()).await?
                == DocumentWrite::Written
            {
                self.dist_stats_cache.put(name, Arc::new(table), Instant::now());
                return Ok(());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
