use super::{CanonicalPackageName, DocumentWrite, RegistryError, Result, Storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The tar integrity and build-trigger verdict derived from one `.tgz`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedTar {
    pub tar_integrity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_build: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDerivedTar {
    Legacy(String),
    Record {
        #[serde(rename = "tarIntegrity")]
        tar_integrity: String,
        #[serde(default, rename = "requiresBuild")]
        requires_build: Option<bool>,
    },
}

impl<'de> Deserialize<'de> for DerivedTar {
    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> std::result::Result<Self, Deserializer::Error> {
        match StoredDerivedTar::deserialize(deserializer)? {
            StoredDerivedTar::Legacy(tar_integrity) => {
                Ok(Self { tar_integrity, requires_build: None })
            }
            StoredDerivedTar::Record { tar_integrity, requires_build } => {
                Ok(Self { tar_integrity, requires_build })
            }
        }
    }
}

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
type Table = Arc<BTreeMap<String, DerivedTar>>;

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

    /// Cache `table` as `name`'s table as of `read_at`, unless the cache
    /// already holds a newer one: a read that started before a record
    /// finished must not replace what the record wrote.
    fn put(&self, name: &CanonicalPackageName, table: Table, read_at: Instant) {
        let mut tables = self.tables.lock().expect("tar integrity cache lock poisoned");
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
    ) -> Result<Arc<BTreeMap<String, DerivedTar>>> {
        if let Some(table) = self.tar_integrity_cache.get(name) {
            return Ok(table);
        }
        let read_at = Instant::now();
        let document = self.tar_integrities.read_document(name).await?;
        let table: Arc<BTreeMap<String, DerivedTar>> = Arc::new(
            document
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default(),
        );
        self.tar_integrity_cache.put(name, Arc::clone(&table), read_at);
        Ok(table)
    }

    /// Record that the `.tgz` with integrity `tgz_integrity` decodes to a tar
    /// with integrity `tar_integrity` and build-trigger verdict `requires_build`.
    /// A record that keeps losing the race to concurrent ones is dropped; the
    /// version's next tar request derives it again.
    pub async fn record_tar_integrity(
        &self,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
        tar_integrity: &str,
        requires_build: Option<bool>,
    ) -> Result<()> {
        if self.tar_integrity_cache
            .get(name)
            .is_some_and(|table| {
                is_tar_entry_recorded(&table, tgz_integrity, tar_integrity, requires_build)
            })
        {
            return Ok(());
        }
        for _ in 0..RECORD_ATTEMPTS {
            let current = self.tar_integrities.read_document_for_update(name).await?;
            let (mut table, version) = match current {
                Some(doc) => {
                    (serde_json::from_slice(&doc.bytes).unwrap_or_default(), Some(doc.version))
                }
                None => (BTreeMap::new(), None),
            };
            if is_tar_entry_recorded(&table, tgz_integrity, tar_integrity, requires_build) {
                return Ok(());
            }
            insert_tar_entry(&mut table, tgz_integrity, tar_integrity, requires_build);
            let bytes = serde_json::to_vec(&table)
                .map_err(|err| RegistryError::Internal {
                    reason: format!("encode tar integrities: {err}"),
                })?;
            let write =
                self.tar_integrities.write_document_if_current(name, &bytes, version.as_ref())
                    .await?;
            if write == DocumentWrite::Written {
                self.tar_integrity_cache.put(name, Arc::new(table), Instant::now());
                return Ok(());
            }
        }
        Ok(())
    }
}

fn is_tar_entry_recorded(
    table: &BTreeMap<String, DerivedTar>,
    tgz_integrity: &str,
    tar_integrity: &str,
    requires_build: Option<bool>,
) -> bool {
    table
        .get(tgz_integrity)
        .is_some_and(|known| {
            known.tar_integrity == tar_integrity
                && (requires_build.is_none() || known.requires_build == requires_build)
        })
}

fn insert_tar_entry(
    table: &mut BTreeMap<String, DerivedTar>,
    tgz_integrity: &str,
    tar_integrity: &str,
    requires_build: Option<bool>,
) {
    let merged_requires_build =
        requires_build.or_else(|| table.get(tgz_integrity).and_then(|known| known.requires_build));
    table.insert(
        tgz_integrity.to_string(),
        DerivedTar {
            tar_integrity: tar_integrity.to_string(),
            requires_build: merged_requires_build,
        },
    );
}

#[cfg(test)]
mod tests;
