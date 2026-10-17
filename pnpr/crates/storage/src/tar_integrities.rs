use super::{CanonicalPackageName, DocumentWrite, RegistryError, Result, Storage};
use std::collections::BTreeMap;

/// The hosted-backend namespace of the tar-integrity tables. A leading `.`
/// keeps it apart from every organization and package name, and out of the
/// hosted inventories.
pub(super) const NAMESPACE: &str = ".tar-integrity";

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
    ) -> Result<BTreeMap<String, String>> {
        let document = self.tar_integrities.read_document(name).await?;
        Ok(document
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default())
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
                return Ok(());
            }
        }
        Ok(())
    }
}
