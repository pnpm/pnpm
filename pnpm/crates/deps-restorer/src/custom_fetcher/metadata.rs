use super::{
    CustomFetchOutcome, CustomFetcherSession, FetchedTarball, InstallPackageBySnapshotError,
    LockfileResolution, ResolvedTarballMetadata, decode_resolution, fetch_custom_tarball,
};
use crate::install_package_by_snapshot::tarball_url_and_integrity;
use pnpm_config::Config;
use pnpm_lockfile::PackageKey;
use pnpm_reporter::Reporter;
use pnpm_tarball::{IngestTarballToStore, TarballPackage};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

impl CustomFetcherSession {
    /// `config` supplies the registry a delegate to a registry resolution is
    /// fetched from when `download` names no URL.
    pub async fn resolve_tarball_metadata<Reporter: self::Reporter>(
        &self,
        download: IngestTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
        config: &Config,
    ) -> Result<ResolvedTarballMetadata, InstallPackageBySnapshotError> {
        let lockfile_dir = PathBuf::from(
            opts.get("lockfileDir")
                .and_then(Value::as_str)
                .unwrap_or(download.requester),
        );
        let (resolution, delegate, fetched) =
            match self.fetch::<Reporter>(download.clone(), original, opts).await? {
                CustomFetchOutcome::Fetched { resolution, tarball } => {
                    (resolution, None, Some(tarball))
                }
                CustomFetchOutcome::Declined(resolution) => (resolution, None, None),
                CustomFetchOutcome::Delegate { resolution, delegate } => {
                    (resolution, Some(delegate), None)
                }
            };
        let source = delegate.as_ref().unwrap_or(&resolution);
        let tarball = if let Some(tarball) = fetched {
            tarball
        } else {
            let Some(tarball) =
                fetch_source::<Reporter>(&download, source, &lockfile_dir, config).await?
            else {
                return Ok(ResolvedTarballMetadata { resolution, manifest: None });
            };
            tarball
        };
        let metadata =
            resolve_archive_metadata((&resolution, source), &tarball, download.package.id).await?;
        self.record_completed_identities(
            download.package.id,
            (&resolution, source),
            &metadata,
            tarball,
        );
        Ok(metadata)
    }

    /// Files the fetch under the resolution-time id and under the
    /// `name@version` its manifest declares. A resolver that reports no
    /// `name@version` leaves the resolution-time id a URL, while the install
    /// pass looks the fetch up by the lockfile key, which is derived from
    /// that manifest.
    fn record_completed_identities(
        &self,
        package_id: &str,
        resolutions: (&LockfileResolution, &LockfileResolution),
        metadata: &ResolvedTarballMetadata,
        tarball: Arc<FetchedTarball>,
    ) {
        self.cache_resolved_tarball(package_id, resolutions, Arc::clone(&tarball));
        let Some(manifest) = &metadata.manifest else { return };
        let (Some(name), Some(version)) = (
            manifest.get("name").and_then(Value::as_str),
            manifest.get("version").and_then(Value::as_str),
        ) else {
            return;
        };
        let name_ver = format!("{name}@{version}");
        if name_ver != package_id {
            self.cache_resolved_tarball(&name_ver, resolutions, tarball);
        }
    }

    fn cache_resolved_tarball(
        &self,
        package_id: &str,
        resolutions: (&LockfileResolution, &LockfileResolution),
        tarball: Arc<FetchedTarball>,
    ) {
        // A delegate can change the archive layout, which the recorded resolution cannot describe.
        if resolutions.0 != resolutions.1
            && [resolutions.0, resolutions.1].iter().any(|resolution| {
                matches!(resolution, LockfileResolution::Tarball(tarball) if tarball.is_git_hosted())
            })
        {
            return;
        }
        let mut completed = self.completed.lock().unwrap();
        completed.insert(
            (package_id.to_owned(), tarball.integrity.to_string()),
            Arc::clone(&tarball),
        );
        // The install pass looks the session up under the lockfile-derived
        // `name@version`, but a pnpmfile `resolvers` hook leaves `name_ver`
        // unset, so the id filed above is the tarball URL and the lookup
        // misses. The fetched manifest names the same package the lockfile
        // records, so file the fetch under its `name@version` as well: one
        // custom fetch per package per install.
        // <https://github.com/pnpm/pnpm/issues/15025>
        if let Some(manifest_id) = manifest_package_id(tarball.manifest.as_ref())
            && manifest_id != package_id
        {
            completed.insert((manifest_id, tarball.integrity.to_string()), tarball);
        }
    }
}

/// The `name@version` a fetched archive's manifest carries: the identity
/// the install pass derives from the lockfile entry of a registry-shaped
/// package.
fn manifest_package_id(manifest: Option<&Value>) -> Option<String> {
    let manifest = manifest?;
    let name = manifest.get("name")?.as_str()?;
    let version = manifest.get("version")?.as_str()?;
    Some(format!("{name}@{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnpm_tarball::{
        ArchiveFetchOptions, ArchiveStoreContext, ArchiveStoreProjection, IngestTarballToStore,
        RetryOpts, TarballPackage,
    };
    use std::{
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    /// A custom fetcher that delegates every package to a local tarball
    /// fixture and counts how often the hook ran.
    struct CountingDelegateFetcher {
        fetches: AtomicUsize,
        tarball_url: String,
        integrity: String,
    }

    #[async_trait::async_trait]
    impl pnpm_hooks::CustomFetcher for CountingDelegateFetcher {
        async fn can_fetch(
            &self,
            _pkg_id: &str,
            _resolution: Value,
        ) -> Result<bool, pnpm_hooks::HookError> {
            Ok(true)
        }

        async fn fetch(
            &self,
            _pkg_id: &str,
            _resolution: Value,
            _opts: Value,
        ) -> Result<Value, pnpm_hooks::HookError> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({
                "delegate": {
                    "tarball": self.tarball_url,
                    "integrity": self.integrity,
                },
            }))
        }
    }

    fn fixture_tarball(name: &str, version: &str) -> Vec<u8> {
        let manifest = serde_json::json!({ "name": name, "version": version }).to_string();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, manifest.as_bytes()).unwrap();
        let tar_bytes = builder.into_inner().unwrap();
        let mut encoder =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn leaked_config(store_dir: &Path) -> &'static pnpm_config::Config {
        let mut config = pnpm_config::Config::new();
        config.store_dir = store_dir.to_path_buf().into();
        Box::leak(Box::new(config))
    }

    fn ingest<'a>(
        config: &'static pnpm_config::Config,
        http_client: &'a pnpm_network::ThrottledClient,
        url: &'a str,
        id: &'a str,
        integrity: Option<&'a ssri::Integrity>,
    ) -> IngestTarballToStore<'a> {
        IngestTarballToStore {
            fetching: ArchiveFetchOptions {
                http_client,
                auth_headers: config.auth_headers.as_ref(),
                retry_opts: RetryOpts { retries: 0, ..Default::default() },
                offline: true,
            },
            package: TarballPackage {
                integrity,
                unpacked_size: None,
                file_count: None,
                url,
                id,
            },
            store: ArchiveStoreContext {
                dir: &config.store_dir,
                index: None,
                index_writer: None,
                verify_integrity: config.verify_store_integrity,
                strict_pkg_content_check: config.strict_store_pkg_content_check,
                verified_files_cache: pnpm_store_dir::SharedVerifiedFilesCache::default(),
                prefetched_cas_paths: None,
            },
            requester: "",
            ignore_file_pattern: None,
            progress_reported: None,
            store_projection: ArchiveStoreProjection::Package { append_manifest: None },
        }
    }

    /// A pnpmfile `resolvers` hook leaves `ResolvedPackageInfo::name_ver`
    /// unset, so the resolve pass files its custom fetch under the tarball
    /// URL while the install pass looks the session up under the
    /// lockfile-derived `name@version`. The fetch must be filed under the
    /// `name@version` read from the fetched manifest too, so the hook runs
    /// once per package per install, not twice.
    /// <https://github.com/pnpm/pnpm/issues/15025>
    #[tokio::test]
    async fn resolve_time_custom_fetch_is_reused_by_the_install_pass() {
        let dir = tempfile::tempdir().unwrap();
        let body = fixture_tarball("foo", "1.0.0");
        let tarball_path = dir.path().join("foo.tgz");
        std::fs::write(&tarball_path, &body).unwrap();
        let integrity = ssri::Integrity::from(&body).to_string();

        let fetcher = Arc::new(CountingDelegateFetcher {
            fetches: AtomicUsize::new(0),
            tarball_url: format!("file:{}", tarball_path.display()),
            integrity: integrity.clone(),
        });
        let session = CustomFetcherSession::new(vec![fetcher.clone()]);

        let config = leaked_config(&dir.path().join("store"));
        let http_client = pnpm_network::ThrottledClient::default();

        // Resolve pass: no `name_ver`, so the session id is the tarball URL.
        let resolve_url = "https://registry.example/foo.tgz";
        let original = LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
            tarball: resolve_url.to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        });
        let metadata = session
            .resolve_tarball_metadata::<pnpm_reporter::SilentReporter>(
                ingest(config, &http_client, resolve_url, resolve_url, None),
                &original,
                serde_json::json!({ "lockfileDir": dir.path() }),
            )
            .await
            .expect("the resolve-time custom fetch succeeds");
        assert_eq!(fetcher.fetches.load(Ordering::SeqCst), 1);
        let manifest = metadata.manifest.expect("the fetched archive has a manifest");
        assert_eq!(manifest["name"], serde_json::json!("foo"));

        // Install pass: the lockfile entry is registry-shaped, so the
        // session is looked up under `name@version`.
        let locked: ssri::Integrity = integrity.parse().unwrap();
        let outcome = session
            .fetch::<pnpm_reporter::SilentReporter>(
                ingest(config, &http_client, resolve_url, "foo@1.0.0", Some(&locked)),
                &metadata.resolution,
                serde_json::json!({}),
            )
            .await
            .expect("the install-time session lookup succeeds");
        assert!(
            matches!(outcome, CustomFetchOutcome::Fetched { .. }),
            "the install pass must reuse the resolve-time fetch instead of running the hook again",
        );
        assert_eq!(
            fetcher.fetches.load(Ordering::SeqCst),
            1,
            "the custom fetcher hook ran twice for one package",
        );
    }
}

async fn fetch_source<Reporter: self::Reporter>(
    download: &IngestTarballToStore<'_>,
    source: &LockfileResolution,
    lockfile_dir: &Path,
    config: &Config,
) -> Result<Option<Arc<FetchedTarball>>, InstallPackageBySnapshotError> {
    let registry_url = registry_delegate_url(download, source, config)?;
    let download = IngestTarballToStore {
        package: TarballPackage {
            url: registry_url.as_deref().unwrap_or(download.package.url),
            ..download.package
        },
        ..download.clone()
    };
    fetch_custom_tarball::<Reporter>(download, source, lockfile_dir).await
}

/// The URL the install pass fetches a registry delegate from, when the caller
/// has none. A custom resolution names no archive, and the install pass then
/// derives the URL from the lockfile key, which for a resolution that reports
/// no `name@version` is the resolver's id. An id that names no package leaves
/// the delegate to the install pass.
fn registry_delegate_url(
    download: &IngestTarballToStore<'_>,
    source: &LockfileResolution,
    config: &Config,
) -> Result<Option<String>, InstallPackageBySnapshotError> {
    if !download.package.url.is_empty() || !matches!(source, LockfileResolution::Registry(_)) {
        return Ok(None);
    }
    let Ok(package_key) = download.package.id.parse::<PackageKey>() else { return Ok(None) };
    let (url, _) = tarball_url_and_integrity(source, &package_key, config)?;
    Ok(Some(url.into_owned()))
}

async fn resolve_archive_metadata(
    resolutions: (&LockfileResolution, &LockfileResolution),
    tarball: &FetchedTarball,
    package_id: &str,
) -> Result<ResolvedTarballMetadata, InstallPackageBySnapshotError> {
    let (resolution, source) = resolutions;
    let subdir = match source {
        LockfileResolution::Tarball(resolution) if resolution.is_git_hosted() => {
            resolution.path.as_deref()
        }
        _ => None,
    };
    let manifest = match subdir {
        Some(subdir) => pnpm_tarball::read_subdir_manifest(&tarball.files_map, subdir)
            .await
            .map_err(InstallPackageBySnapshotError::DownloadTarball)?,
        None => tarball.manifest.clone(),
    }
    .map(Arc::new);
    // A commit-addressed archive is anchored by its SHA, and a custom
    // resolution by whatever identity its fetcher defines. Neither is named by
    // the hash the bytes happen to yield, so recording that hash would replace
    // an identity pacquet does not own.
    let self_addressed = match resolution {
        LockfileResolution::Tarball(resolution) => {
            pnpm_lockfile::is_git_hosted_tarball_url(&resolution.tarball)
        }
        LockfileResolution::Custom(_) => true,
        _ => false,
    };
    let resolution = if self_addressed {
        resolution.clone()
    } else {
        decode_resolution(serde_json::json!(resolution), Some(&tarball.integrity), package_id)?
    };
    Ok(ResolvedTarballMetadata { resolution, manifest })
}
