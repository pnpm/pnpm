//! Warming the store from a package tarball on disk, such as one a
//! sandboxed build downloaded before it lost network access.

use super::{AddOne, ArchiveToAdd, Reporter};
use miette::{IntoDiagnostic, WrapErr};
use pnpm_store_dir::store_index_key;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::path::Path;

impl AddOne<'_> {
    /// Ingest the tarball at `path`, then make its row reachable under every
    /// key an install of the registry package it holds may look up:
    /// `<integrity>\t<name>@<version>` for the tarball's sha512 and sha1 and
    /// for the tar it holds. Which one a lockfile records depends on the
    /// package's age and on `lockfile.integrityTarget`.
    ///
    /// Each alias names a hash of the bytes that were ingested, so a lockfile
    /// entry reaches the row only when it pins exactly this tarball.
    pub(super) async fn add_downloaded_tarball<Reporter: self::Reporter>(
        &self,
        path: &Path,
        archive: &ArchiveToAdd<'_>,
    ) -> miette::Result<()> {
        let digests = FileDigests::read(path).await?;
        // Ingestion verifies the sha512, which ties the sha1 read alongside
        // it to the ingested bytes.
        let fetched = self
            .ingestion(archive, Some(&digests.sha512))
            .fetch_and_extract::<Reporter>()
            .await
            .map_err(miette::Report::new)?;
        let Some(registry_id) = registry_package_id(fetched.manifest.as_ref()) else {
            return Ok(());
        };
        let row_key = store_index_key(&fetched.integrity.to_string(), archive.id);
        [Some(&digests.sha512), Some(&digests.sha1), fetched.tar_integrity.as_ref()]
            .into_iter()
            .flatten()
            .map(|integrity| store_index_key(&integrity.to_string(), &registry_id))
            .filter(|alias| *alias != row_key)
            .for_each(|alias| self.store.index_writer.queue_alias(alias, row_key.clone()));
        Ok(())
    }
}

struct FileDigests {
    sha512: Integrity,
    sha1: Integrity,
}

impl FileDigests {
    async fn read(path: &Path) -> miette::Result<Self> {
        let bytes = tokio::fs::read(path).await
            .into_diagnostic()
            .wrap_err_with(|| format!("read the tarball {}", path.display()))?;
        let digest = |algorithm| {
            IntegrityOpts::new()
                .algorithm(algorithm)
                .chain(&bytes)
                .result()
        };
        Ok(FileDigests { sha512: digest(Algorithm::Sha512), sha1: digest(Algorithm::Sha1) })
    }
}

/// The id a registry resolution of the package this manifest names carries.
fn registry_package_id(manifest: Option<&serde_json::Value>) -> Option<String> {
    let manifest = manifest?;
    let name = manifest.get("name")?.as_str()?;
    let version = manifest.get("version")?.as_str()?;
    Some(format!("{name}@{version}"))
}
