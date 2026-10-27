//! Warming the store from a package tarball on disk, such as one a
//! sandboxed build downloaded before it lost network access.

use super::{AddOne, ArchiveToAdd, Reporter};
use miette::{IntoDiagnostic, WrapErr};
use pnpm_store_dir::store_index_key;
use pnpm_tarball::local_file_tarball_path;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::path::{Path, PathBuf};

/// The `resolved_via` of a `file:` tarball the local resolver resolved.
const LOCAL_FILESYSTEM: &str = "local-filesystem";

/// The tarball on disk a resolution names, when the local resolver made it.
///
/// A registry resolution can carry a `file:` tarball URL too, but it pins its
/// own integrity, which the registry ingestion path verifies. The local
/// resolver reads a relative path from `lockfile_dir`, which `--dir` can set
/// apart from the working directory, so the path is joined to it.
pub(super) fn tarball_on_disk(
    resolved_via: &str,
    package_url: &str,
    lockfile_dir: &Path,
) -> Option<PathBuf> {
    if resolved_via != LOCAL_FILESYSTEM {
        return None;
    }
    local_file_tarball_path(package_url).map(|path| lockfile_dir.join(path))
}

impl AddOne<'_> {
    /// Ingest the tarball at `path`, then make its row reachable under every
    /// key an install of the registry package it holds may look up:
    /// `<integrity>\t<name>@<version>` for the tarball's sha512 and sha1 and
    /// for the tar it holds. Which one a lockfile records depends on the
    /// package's age and on `lockfile.integrityTarget`. A package from a
    /// registry reached through a prefix may be keyed
    /// `<name>@<prefix>:<version>`, and the tarball does not say which
    /// registry it came from, so every declared and built-in prefix gets
    /// aliases.
    ///
    /// Each alias names a hash of the bytes that were ingested, so a lockfile
    /// entry reaches the row only when it pins exactly this tarball.
    pub(super) async fn add_downloaded_tarball<Reporter: self::Reporter>(
        &self,
        path: &Path,
        archive: &ArchiveToAdd<'_>,
    ) -> miette::Result<()> {
        let path = std::path::absolute(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("resolve the tarball path {}", path.display()))?;
        let digests = FileDigests::read(&path).await?;
        // Ingestion reads the URL against the working directory, so it gets
        // the path the digests were read from.
        let url = url::Url::from_file_path(&path)
            .map_err(|()| miette::miette!("{} is not a file URL path", path.display()))?
            .to_string();
        let archive = &ArchiveToAdd { url: &url, ..*archive };
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
        let prefixes = self.config.merged_registries_by_prefix();
        let prefixes = prefixes.keys().map(String::as_str);
        for registry_id in registry_package_ids(fetched.manifest.as_ref(), prefixes) {
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
/// package content check accepts either spelling. Each of `prefixes` adds
/// `<name>@<prefix>:<version>`, with the version as semver writes it.
fn registry_package_ids<'p>(
    manifest: Option<&serde_json::Value>,
    prefixes: impl IntoIterator<Item = &'p str>,
) -> Vec<String> {
    let Some((name, version)) = manifest.and_then(|manifest| {
        Some((manifest.get("name")?.as_str()?, manifest.get("version")?.as_str()?))
    }) else {
        return Vec::new();
    };
    let mut ids = vec![format!("{name}@{version}")];
    let Ok(parsed) = node_semver::Version::parse(version) else { return ids };
    if parsed.to_string() != version {
        ids.push(format!("{name}@{parsed}"));
    }
    ids.extend(
        prefixes
            .into_iter()
            .map(|prefix| format!("{name}@{prefix}:{parsed}")),
    );
    ids
}

#[cfg(test)]
mod tests;
