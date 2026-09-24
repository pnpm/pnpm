//! Reading what a tarball resolution does not describe on its own.
//!
//! A resolver hands back a [`ResolveResult`] whose resolution may carry
//! no integrity for the lockfile to record, no manifest for the
//! dependency walk to read the package's children from, or neither:
//! `manifest` is optional in the pnpmfile `resolvers` contract. Both
//! answers live in the archive's bytes, so one read settles both and
//! every edge that resolves to the same content shares it.

use super::PrefetchingResolver;
use pnpm_deps_restorer::{
    CustomFetcherSession, ResolvedTarballMetadata, local_file_tarball_install_url,
};
use pnpm_lockfile::{LockfileResolution, is_git_hosted_tarball_url};
use pnpm_reporter::{Reporter, SilentReporter};
use pnpm_resolving_resolver_base::{ResolveError, ResolveResult};
use pnpm_tarball::{FetchTarballForResolution, package_mem_cache_key};
use std::{path::Path, sync::Arc};

impl<Reporter: self::Reporter + 'static> PrefetchingResolver<Reporter> {
    /// Fill in whichever of a resolution's integrity and manifest the
    /// archive has to answer for, in one read. [`MissingTarballMetadata`]
    /// decides which of them those are, and which shapes are read at all.
    pub(super) async fn populate_missing_tarball_metadata(
        &self,
        result: &mut ResolveResult,
        lockfile_dir: &Path,
    ) -> Result<(), ResolveError> {
        let Some((missing, tarball)) =
            MissingTarballMetadata::of(result, self.ctx.policy.custom_session.is_some())
        else {
            return Ok(());
        };
        let metadata = match self.read_archive_once(result, tarball, lockfile_dir).await {
            Ok(metadata) => metadata,
            Err(err)
                if is_missing_local_tarball(&err)
                    && tarball.is_some_and(|tarball| {
                        tarball.integrity.is_some() && tarball.tarball.starts_with("file:")
                    }) =>
            {
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        // A custom resolution is the resolver's, and the read only interprets
        // it, so there is nothing for the archive to name better. Taking the
        // fetcher's copy would also carry the scratch fields a `canFetch` left
        // on the object into the lockfile, since `decode_resolution` strips
        // those only from resolutions that have no `type`.
        if self.ctx.policy.custom_session.is_some()
            && !matches!(result.resolution, LockfileResolution::Custom(_))
        {
            // A fetcher can select different content for the same URL, and
            // the manifest below was read out of whatever it chose. Record
            // the resolution naming those bytes, not the one it replaced.
            result.resolution = metadata.resolution.clone();
        } else if missing.integrity
            && let LockfileResolution::Tarball(tarball) = &mut result.resolution
        {
            // An unpinned read is shared by URL alone, since the hash is what
            // it is there to learn; each edge keeps its other fields.
            tarball.integrity = metadata.resolution.integrity().cloned();
        }
        if missing.manifest
            && let Some(manifest) = metadata.manifest
        {
            result.package.manifest = Some(manifest);
        }
        Ok(())
    }

    /// Read the archive once per distinct content and share the result
    /// with every other edge that resolves to it. Concurrent first-callers
    /// park on the same [`tokio::sync::OnceCell`] rather than both downloading.
    async fn read_archive_once(
        &self,
        result: &ResolveResult,
        tarball: Option<&pnpm_lockfile::TarballResolution>,
        lockfile_dir: &Path,
    ) -> Result<ResolvedTarballMetadata, ResolveError> {
        // A resolution that names no archive leaves the URL to the fetcher that
        // claims it, the way the install pass does for the same resolution.
        let package_url = tarball.map_or_else(String::new, |tarball| {
            local_file_tarball_install_url(tarball.tarball.as_str().into(), lockfile_dir)
                .into_owned()
        });
        // Scope credentials are selected from `name@version` when the
        // resolver knows it; direct URL tarballs fall back to URL identity,
        // and a resolution without one is named by the resolver's own id.
        let package_id = match result.package.name_ver.as_ref() {
            Some(name_ver) => format!("{}@{}", name_ver.name, name_ver.suffix),
            None if tarball.is_some() => package_url.clone(),
            None => result.id.as_str().to_owned(),
        };
        let cache_key = self.tarball_metadata_cache_key(result, tarball, &package_id)?;
        let cell = Arc::clone(&self.tarball_metadata_cache.entry(cache_key).or_default());
        cell.get_or_try_init(|| async {
            match (self.ctx.policy.custom_session.as_ref(), tarball) {
                (Some(session), _) => {
                    self.read_archive_by_custom_fetcher(
                        session,
                        result,
                        (&package_url, &package_id),
                        lockfile_dir,
                    )
                    .await
                }
                (None, Some(tarball)) => {
                    self.read_archive(tarball, &package_url, &package_id).await
                }
                // Only a fetcher hook can read an archive the resolution does
                // not name, and `MissingTarballMetadata::of` reports such a
                // resolution only when one is configured.
                (None, None) => Ok(ResolvedTarballMetadata {
                    resolution: result.resolution.clone(),
                    manifest: None,
                }),
            }
        })
        .await
        .cloned()
    }

    /// Custom fetchers can choose different content for the same URL for
    /// different packages. The native key carries the pinned hash as well
    /// as the URL, so a read that verifies is never served the bytes of
    /// one that could not, and the network policy the read runs under,
    /// since that is what the extraction it publishes is keyed by. An
    /// unpinned read names neither: the hash is what it is there to learn,
    /// and a revision cannot be recorded without one.
    ///
    /// Each key leads with its kind and separates its parts with a tab,
    /// which neither a URL, an integrity nor a package id can contain, so
    /// no key of one kind can spell a key of another.
    pub(super) fn tarball_metadata_cache_key(
        &self,
        result: &ResolveResult,
        tarball: Option<&pnpm_lockfile::TarballResolution>,
        package_id: &str,
    ) -> Result<String, ResolveError> {
        let Some(tarball) = tarball.filter(|_| self.ctx.policy.custom_session.is_none()) else {
            return Ok(format!(
                "custom\t{package_id}\t{}",
                serde_json::to_string(&result.resolution)?,
            ));
        };
        Ok(if tarball.path.is_some() {
            format!("subdirectory\t{}", serde_json::to_string(tarball)?)
        } else {
            match tarball.integrity.as_ref() {
                Some(integrity) => {
                    let policy = if tarball.revision.is_some() { "revision" } else { "direct" };
                    format!("pinned\t{policy}\t{integrity}\t{}", tarball.tarball)
                }
                None => format!("unpinned\t{}", tarball.tarball),
            }
        })
    }

    /// Read a tarball through the pnpmfile's custom fetcher, which may
    /// select different content for the same URL.
    async fn read_archive_by_custom_fetcher(
        &self,
        session: &CustomFetcherSession,
        result: &ResolveResult,
        package: (&str, &str),
        lockfile_dir: &Path,
    ) -> Result<ResolvedTarballMetadata, ResolveError> {
        let (package_url, package_id) = package;
        let download = self.ctx.tarball_download(package_url, package_id, None, None, None);
        let opts = serde_json::json!({
            "pkg": result.package.name_ver.as_ref().map_or_else(
                || serde_json::json!({}),
                |nv| serde_json::json!({
                    "name": nv.name.to_string(), "version": nv.suffix.to_string(),
                }),
            ),
            "lockfileDir": lockfile_dir,
            "readManifest": true,
            "filesIndexFile": pnpm_store_dir::pick_store_index_key(
                None, false, package_id, !self.ctx.policy.ignore_scripts,
            ),
        });
        session
            .resolve_tarball_metadata::<Reporter>(download, &result.resolution, opts)
            .await
            .map_err(|error| Box::new(error) as ResolveError)
    }

    /// Read a tarball by fetching it into the store, and share the
    /// extraction with the install pass through the mem cache.
    pub(super) async fn read_archive(
        &self,
        tarball: &pnpm_lockfile::TarballResolution,
        package_url: &str,
        package_id: &str,
    ) -> Result<ResolvedTarballMetadata, ResolveError> {
        let revision_addressed = tarball.revision.is_some();
        // A pinned resolution names the archive before the fetch, so claim the
        // download now. A concurrent edge whose own resolution needs no read
        // would otherwise reach `maybe_kickoff_download` while this fetch is in
        // flight and spend a second request on the same archive, which a
        // revision's one-GET protocol does not allow.
        if let Some(integrity) = tarball.integrity.as_ref() {
            self.claim_download(package_url, integrity, revision_addressed);
        }
        let resolved = FetchTarballForResolution {
            http_client: &self.ctx.fetching.http_client,
            store_dir: self.ctx.store.dir,
            store_index_writer: self.ctx.store.index_writer.clone(),
            package: pnpm_tarball::TarballPackage {
                // A resolution that already pins a hash is only read here
                // for its manifest, and an unchecked archive would put
                // attacker content in charge of the dependency walk.
                integrity: tarball.integrity.as_ref(),
                unpacked_size: None,
                file_count: None,
                url: package_url,
                id: package_id,
            },
            auth_headers: &self.ctx.fetching.auth_headers,
            retry_opts: self.ctx.fetching.retry_opts,
            manifest_subdir: tarball.path.as_deref(),
            revision_addressed,
        }
        .run::<SilentReporter>(Some(&self.ctx.mem_cache))
        .await
        .map_err(|err| Box::new(err) as ResolveError)?;
        // The read published its extraction under the hash the resolution
        // below records, so the install pass finds it there instead of
        // downloading the archive a second time. Claiming that identity keeps
        // the prefetch path off it too; for an unpinned archive this is the
        // first point at which the hash that names it is known.
        self.claim_download(package_url, &resolved.integrity, revision_addressed);
        let mut resolution = tarball.clone();
        if !is_git_hosted_tarball_url(&tarball.tarball) {
            resolution.integrity = Some(resolved.integrity);
        } else if tarball.integrity.is_none() {
            self.share_commit_addressed_archive(
                package_url,
                &resolved.integrity,
                revision_addressed,
            );
        }
        Ok::<_, ResolveError>(ResolvedTarballMetadata {
            resolution: LockfileResolution::Tarball(resolution),
            manifest: resolved.manifest.map(Arc::new),
        })
    }
    pub(super) fn share_commit_addressed_archive(
        &self,
        url: &str,
        integrity: &ssri::Integrity,
        revision_addressed: bool,
    ) {
        let Some(archive) = self.ctx.mem_cache
            .get(&package_mem_cache_key(url, Some(integrity), revision_addressed))
            .map(|entry| Arc::clone(entry.value()))
        else {
            // A concurrent pinned fetch can fail and evict the shared entry.
            return;
        };
        self.ctx.mem_cache
            .entry(package_mem_cache_key(url, None, revision_addressed))
            .or_insert(archive);
    }
}

/// Which halves of a tarball resolution the resolver left for the
/// archive itself to supply.
#[derive(Clone, Copy)]
struct MissingTarballMetadata {
    integrity: bool,
    manifest: bool,
}

impl MissingTarballMetadata {
    /// Skip complete resolutions; commit-addressed archives need no new integrity.
    ///
    /// A custom resolution names no archive, so its manifest is only
    /// readable through the fetcher that claims it, and its integrity is that
    /// fetcher's to define rather than the archive's to yield.
    fn of(
        result: &ResolveResult,
        custom_fetchers: bool,
    ) -> Option<(Self, Option<&pnpm_lockfile::TarballResolution>)> {
        let LockfileResolution::Tarball(tarball) = &result.resolution else {
            let recoverable = custom_fetchers
                && matches!(result.resolution, LockfileResolution::Custom(_))
                && result.package.manifest.is_none();
            return recoverable.then_some((
                MissingTarballMetadata { integrity: false, manifest: true },
                None,
            ));
        };
        // git-hosted tarballs are anchored by their commit SHA, not an integrity. Detect
        // them by URL, NOT by the `git_hosted` flag: the flag is tamper-prone lockfile
        // input, so trusting it would let a forged `git_hosted: true` on an arbitrary URL
        // skip the integrity computation. A real git-hosted archive (codeload/gitlab/
        // bitbucket) always has a matching URL.
        let missing = MissingTarballMetadata {
            integrity: tarball.integrity.is_none()
                && !is_git_hosted_tarball_url(&tarball.tarball)
                && (!tarball.tarball.starts_with("file:")
                    || result.package.manifest.is_none()),
            manifest: result.package.manifest.is_none(),
        };
        (missing.integrity || missing.manifest).then_some((missing, Some(tarball)))
    }
}

fn is_missing_local_tarball(err: &ResolveError) -> bool {
    if let Some(tarball_err) = err.downcast_ref::<pnpm_tarball::TarballError>() {
        matches!(
            tarball_err,
            pnpm_tarball::TarballError::ReadLocalTarball { source, .. }
                if source.kind() == std::io::ErrorKind::NotFound,
        )
    } else {
        false
    }
}
