use super::{CanonicalPackageName, DocumentWrite, RegistryError, Result, Storage};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The hosted-backend namespace of the tar-integrity tables. A leading `.`
/// keeps it apart from every organization and package name, and out of the
/// hosted inventories.
pub(super) const NAMESPACE: &str = ".tar-integrity";

/// How long a replica serves a table it read before reading it again, so a
/// hash another replica recorded shows up in this replica's metadata after at
/// most this long.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Tables kept in memory before the cache starts over.
const CACHE_CAPACITY: usize = 4096;

/// The tables this process read or wrote recently, keyed by package name, so
/// a metadata request does not cost a hosted-backend read.
#[derive(Debug, Default)]
pub(super) struct TarIntegrityCache {
    tables: Mutex<HashMap<String, CachedTable>>,
}

/// A tar-integrity table, keyed by the `.tgz` integrity each entry was
/// derived from.
type Table = Arc<BTreeMap<String, String>>;

#[derive(Debug)]
struct CachedTable {
    read_at: Instant,
    table: Table,
}

impl TarIntegrityCache {
    fn get(&self, name: &CanonicalPackageName) -> Option<Table> {
        let tables = self.tables.lock().expect("tar integrity cache lock poisoned");
        let cached = tables.get(name.as_str())?;
        (cached.read_at.elapsed() < CACHE_TTL).then(|| Arc::clone(&cached.table))
    }

    fn put(&self, name: &CanonicalPackageName, table: Table) {
        let mut tables = self.tables.lock().expect("tar integrity cache lock poisoned");
        if tables.len() >= CACHE_CAPACITY {
            tables.clear();
        }
        tables.insert(name.as_str().to_string(), CachedTable { read_at: Instant::now(), table });
    }
}

/// Conflicting writes [`Storage::record_tar_integrity`] retries before
/// leaving the entry to be derived again.
const RECORD_ATTEMPTS: usize = 5;

impl Storage {
    /// The tar integrities derived so far for `name`'s versions, keyed by the
    /// `dist.integrity` of the `.tgz` each was derived from.
    ///
    /// The table lives in the hosted backend, so every replica shares it, and
    /// it is shared by every organization: each value is a function of its
    /// key, so it says nothing a reader of the version's metadata could not
    /// compute. An unreadable table reads as empty.
    pub async fn read_tar_integrities(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Arc<BTreeMap<String, String>>> {
        if let Some(table) = self.tar_integrity_cache.get(name) {
            return Ok(table);
        }
        let document = self.tar_integrities.read_document(name).await?;
        let table: Arc<BTreeMap<String, String>> = Arc::new(
            document
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default(),
        );
        self.tar_integrity_cache.put(name, Arc::clone(&table));
        Ok(table)
    }

    /// Record that the `.tgz` with integrity `tgz_integrity` decodes to a tar
    /// with integrity `tar_integrity`. A record that keeps losing the race to
    /// concurrent ones is dropped; the version's next tar request derives it
    /// again.
    pub async fn record_tar_integrity(
        &self,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
        tar_integrity: &str,
    ) -> Result<()> {
        for _ in 0..RECORD_ATTEMPTS {
            let current = self.tar_integrities.read_document_for_update(name).await?;
            let (mut table, version) = match current {
                Some(document) => (
                    serde_json::from_slice::<BTreeMap<String, String>>(&document.bytes)
                        .unwrap_or_default(),
                    Some(document.version),
                ),
                None => (BTreeMap::new(), None),
            };
            if table
                .get(tgz_integrity)
                .is_some_and(|known| known == tar_integrity)
            {
                return Ok(());
            }
            table.insert(tgz_integrity.to_string(), tar_integrity.to_string());
            let bytes = serde_json::to_vec(&table)
                .map_err(|err| RegistryError::Internal {
                    reason: format!("encode tar integrities: {err}"),
                })?;
            let write =
                self.tar_integrities.write_document_if_current(name, &bytes, version.as_ref())
                    .await?;
            if write == DocumentWrite::Written {
                self.tar_integrity_cache.put(name, Arc::new(table));
                return Ok(());
            }
        }
        Ok(())
    }
}
