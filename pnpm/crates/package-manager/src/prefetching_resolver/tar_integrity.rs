//! Pinning the tar of a registry package under `lockfile.integrityTarget: tar`.

use super::PrefetchingResolver;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_lockfile::{IntegrityTarget, LockfileResolution, TarballResolution};
use pnpm_reporter::{Reporter, SilentReporter};
use pnpm_resolving_resolver_base::{ResolveError, ResolveResult};
use pnpm_store_dir::{StoreIndex, store_index_key};
use pnpm_tarball::{FetchTarballForResolution, IntegrityKind, package_mem_cache_key};
use ssri::Integrity;
use std::sync::Arc;

/// A package's tar hash is needed while a custom fetcher serves its archive.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot learn the tar hash of {package_id}, because a custom fetcher serves it. Remove lockfile.integrityTarget, or install the package once without the fetcher."
)]
#[diagnostic(code(ERR_PNPM_INTEGRITY_TARGET_WITH_CUSTOM_FETCHER))]
struct TarHashBehindCustomFetcherError {
    #[error(not(source))]
    package_id: String,
}

impl<Reporter: self::Reporter + 'static> PrefetchingResolver<Reporter> {
    /// Replace the integrity of a registry package's tarball resolution with
    /// the hash of its tar, when the lockfile pins tars.
    ///
    /// The resolver hands back either the registry's hash of the resource or
    /// a hash a lockfile already pinned, which is the tar's. A store row
    /// reachable under it records the tar hash of what it holds, so a
    /// version installed before needs no download. Otherwise the archive is
    /// read once, checked against the integrity as either hash, and its tar
    /// hashed.
    pub(super) async fn pin_tar_integrity(
        &self,
        result: &mut ResolveResult,
    ) -> Result<(), ResolveError> {
        if self.ctx.config.lockfile_integrity_target != IntegrityTarget::Tar {
            return Ok(());
        }
        let Some(name_ver) = result.package.name_ver.as_ref() else { return Ok(()) };
        let LockfileResolution::Tarball(tarball) = &result.resolution else { return Ok(()) };
        if result.resolution.integrity_target(IntegrityTarget::Tar) != IntegrityTarget::Tar
            || tarball.revision.is_some()
            || tarball.path.is_some()
            || tarball.tarball.starts_with("file:")
        {
            return Ok(());
        }
        let Some(integrity) = tarball.integrity.clone() else { return Ok(()) };
        let package_id = format!("{}@{}", name_ver.name, name_ver.suffix);
        let tar = match self.stored_tar_integrity(&integrity, &package_id).await {
            Some(tar) => tar,
            // A custom fetcher owns the download, so pnpm cannot read the
            // archive past it.
            None if self.ctx.policy.custom_session.is_some() => {
                return Err(Box::new(TarHashBehindCustomFetcherError { package_id }));
            }
            None => self.read_tar_integrity(tarball, &integrity, &package_id).await?,
        };
        if let LockfileResolution::Tarball(tarball) = &mut result.resolution {
            tarball.integrity = Some(tar);
        }
        Ok(())
    }

    /// The tar integrity the store row reachable under `integrity` records.
    async fn stored_tar_integrity(
        &self,
        integrity: &Integrity,
        package_id: &str,
    ) -> Option<Integrity> {
        let index = Arc::clone(self.ctx.store.index.as_ref()?);
        let key = store_index_key(&integrity.to_string(), package_id);
        let row = tokio::task::spawn_blocking(move || StoreIndex::get_shared(&index, &key))
            .await
            .ok()?
            .ok()??;
        row.tar_integrity?.parse().ok()
    }

    /// Make the extraction published under `pinned` reachable under `tar` too.
    fn share_archive(&self, url: &str, pinned: &Integrity, tar: &Integrity) {
        let Some(archive) = self.ctx.mem_cache
            .get(&package_mem_cache_key(url, Some(pinned), false))
            .map(|entry| Arc::clone(entry.value()))
        else {
            return;
        };
        self.ctx.mem_cache
            .entry(package_mem_cache_key(url, Some(tar), false))
            .or_insert(archive);
    }

    /// Read the archive once per distinct content to learn its tar
    /// integrity, sharing the read with every edge that resolves to it.
    async fn read_tar_integrity(
        &self,
        tarball: &TarballResolution,
        integrity: &Integrity,
        package_id: &str,
    ) -> Result<Integrity, ResolveError> {
        let cache_key = format!("tar\t{integrity}\t{}", tarball.tarball);
        let cell = Arc::clone(&self.tar_integrity_cache.entry(cache_key).or_default());
        cell.get_or_try_init(|| async {
            self.claim_download(&tarball.tarball, integrity, false);
            let resolved = FetchTarballForResolution {
                http_client: &self.ctx.fetching.http_client,
                store: self.ctx.store.resolution_store(),
                package: pnpm_tarball::TarballPackage {
                    integrity_kind: IntegrityKind::ResourceOrTar,
                    integrity: Some(integrity),
                    unpacked_size: None,
                    file_count: None,
                    url: &tarball.tarball,
                    id: package_id,
                },
                auth_headers: &self.ctx.fetching.auth_headers,
                retry_opts: self.ctx.fetching.retry_opts,
                tar_limits: self.ctx.fetching.tar_limits,
                manifest_subdir: None,
                revision_addressed: false,
            }
            .run::<SilentReporter>(Some(&self.ctx.mem_cache))
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;
            // The install pass looks the extraction up by the tar integrity
            // the lockfile now records.
            self.claim_download(&tarball.tarball, &resolved.integrity, false);
            self.share_archive(&tarball.tarball, integrity, &resolved.integrity);
            Ok::<_, ResolveError>(resolved.integrity)
        })
        .await
        .cloned()
    }
}
