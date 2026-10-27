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
        let row_key = store_index_key(&fetched.integrity.to_string(), archive.id);
        let integrities =
            [Some(&digests.sha512), Some(&digests.sha1), fetched.tar_integrity.as_ref()];
        for registry_id in registry_package_ids(fetched.manifest.as_ref()) {
            integrities
                .iter()
                .flatten()
                .map(|integrity| store_index_key(&integrity.to_string(), &registry_id))
                .filter(|alias| *alias != row_key)
                .for_each(|alias| self.store.index_writer.queue_alias(alias, row_key.clone()));
        }
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

/// The ids a registry resolution of the package this manifest names may
/// carry: `<name>@<version>` with the version as the manifest spells it and,
/// when that differs, as semver writes it (`v1.0.0` is `1.0.0`). The store's
/// package content check accepts either spelling.
fn registry_package_ids(manifest: Option<&serde_json::Value>) -> Vec<String> {
    let Some((name, version)) = manifest.and_then(|manifest| {
        Some((manifest.get("name")?.as_str()?, manifest.get("version")?.as_str()?))
    }) else {
        return Vec::new();
    };
    let mut ids = vec![format!("{name}@{version}")];
    if let Ok(parsed) = node_semver::Version::parse(version)
        && parsed.to_string() != version
    {
        ids.push(format!("{name}@{parsed}"));
    }
    ids
}

#[cfg(test)]
mod tests;
