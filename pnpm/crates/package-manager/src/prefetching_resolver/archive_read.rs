//! Reading what a tarball resolution does not describe on its own.
//!
//! A resolver hands back a [`ResolveResult`] whose resolution may carry
//! no integrity for the lockfile to record, no manifest for the
//! dependency walk to read the package's children from, or neither:
//! `manifest` is optional in the pnpmfile `resolvers` contract. Both
//! answers live in the archive's bytes, so one read settles both and
//! every edge that resolves to the same content shares it.

use super::PrefetchingResolver;
use pnpm_deps_restorer::{CustomFetcherSession, ResolvedTarballMetadata};
use pnpm_lockfile::{LockfileResolution, is_git_hosted_tarball_url};
use pnpm_reporter::{Reporter, SilentReporter};
use pnpm_resolving_resolver_base::{ResolveError, ResolveResult};
use pnpm_tarball::FetchTarballForResolution;
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
        let Some((missing, tarball)) = MissingTarballMetadata::of(result) else {
            return Ok(());
        };
        let metadata = self.read_archive_once(result, tarball, lockfile_dir).await?;
        if self.ctx.policy.custom_session.is_some() {
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
        tarball: &pnpm_lockfile::TarballResolution,
        lockfile_dir: &Path,
    ) -> Result<ResolvedTarballMetadata, ResolveError> {
        let package_url = tarball.tarball.clone();
        // Scope credentials are selected from `name@version` when the
        // resolver knows it; direct URL tarballs fall back to URL identity.
        let package_id = result.package.name_ver
            .as_ref()
            .map_or_else(|| package_url.clone(), |nv| format!("{}@{}", nv.name, nv.suffix));
        let cache_key = self.tarball_metadata_cache_key(result, tarball, &package_id)?;
        let cell = Arc::clone(&self.tarball_metadata_cache.entry(cache_key).or_default());
        cell.get_or_try_init(|| async {
            match self.ctx.policy.custom_session.as_ref() {
                Some(session) => {
                    self.read_archive_by_custom_fetcher(
                        session,
                        result,
                        (&package_url, &package_id),
                        lockfile_dir,
                    )
                    .await
                }
                None => self.read_archive(tarball, &package_url, &package_id).await,
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
        tarball: &pnpm_lockfile::TarballResolution,
        package_id: &str,
    ) -> Result<String, ResolveError> {
        Ok(if self.ctx.policy.custom_session.is_some() {
            format!("custom\t{package_id}\t{}", serde_json::to_string(&result.resolution)?)
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
    async fn read_archive(
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
            // git-hosted archives, the sole subdirectory-bearing shape,
            // are filtered out above.
            manifest_subdir: None,
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
        resolution.integrity = Some(resolved.integrity);
        Ok::<_, ResolveError>(ResolvedTarballMetadata {
            resolution: LockfileResolution::Tarball(resolution),
            manifest: resolved.manifest.map(Arc::new),
        })
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
    /// `None` when the resolution needs nothing from the archive, and for
    /// the shapes read here by neither hash nor manifest: a non-tarball
    /// resolution, a `file:` archive, or a git-hosted one. Those belong to
    /// the local and git resolvers, which settle both fields themselves;
    /// a custom resolver that emits one of them and no manifest is
    /// <https://github.com/pnpm/pnpm/issues/15016>.
    fn of(result: &ResolveResult) -> Option<(Self, &pnpm_lockfile::TarballResolution)> {
        let LockfileResolution::Tarball(tarball) = &result.resolution else {
            return None;
        };
        // git-hosted tarballs are anchored by their commit SHA, not an integrity. Detect
        // them by URL, NOT by the `git_hosted` flag: the flag is tamper-prone lockfile
        // input, so trusting it would let a forged `git_hosted: true` on an arbitrary URL
        // skip the integrity computation. A real git-hosted archive (codeload/gitlab/
        // bitbucket) always has a matching URL.
        if is_git_hosted_tarball_url(&tarball.tarball) || tarball.tarball.starts_with("file:") {
            return None;
        }
        let missing = MissingTarballMetadata {
            integrity: tarball.integrity.is_none(),
            manifest: result.package.manifest.is_none(),
        };
        (missing.integrity || missing.manifest).then_some((missing, tarball))
    }
}
