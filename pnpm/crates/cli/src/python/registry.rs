use super::host::{self, Interpreter, Wheel, WheelMetadata};
use futures_util::{StreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_config::Config;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_python_resolver::{LockedPackage, Packages, candidates_from_page, wheel_identity};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{ArchiveStoreProjection, IngestZipArchiveToStore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use tokio::io::AsyncReadExt;
use url::Url;

const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = MAX_INDEX_BYTES + 64 * 1024;

#[derive(Serialize, Deserialize)]
struct CachedIndex {
    url: Url,
    body: Box<serde_json::value::RawValue>,
}

pub(super) struct Registry<'a> {
    pub(super) config: &'static Config,
    pub(super) client: &'a ThrottledClient,
    pub(super) auth: AuthHeaders,
    pub(super) index: Url,
    pub(super) interpreter: &'a Interpreter,
    pub(super) store_index: Option<SharedReadonlyStoreIndex>,
    pub(super) writer: Arc<StoreIndexWriter>,
    pub(super) verified: SharedVerifiedFilesCache,
    /// What resolution reads: the candidates each distribution offers and
    /// the metadata of the wheels it has looked at.
    pub(super) packages: Packages,
    /// What installing reads: the store paths of every wheel downloaded so
    /// far, beside the interpreter's full report on it.
    pub(super) wheels: BTreeMap<(PackageName, Version), Wheel>,
}

impl Registry<'_> {
    pub(super) async fn fetch_index(&mut self, name: &PackageName) -> Result<()> {
        let index_url = self.index.join(&format!("{name}/")).into_diagnostic()?;
        let cache = self
            .config
            .cache_dir
            .join("python-index-v2")
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(index_url.as_str())));
        let cached = if self.config.offline {
            let file =
                tokio::fs::File::open(&cache).await.into_diagnostic().wrap_err_with(|| {
                    format!("Python index for {name} is not cached for offline resolution")
                })?;
            let mut contents = Vec::new();
            file.take(MAX_CACHE_BYTES as u64 + 1)
                .read_to_end(&mut contents)
                .await
                .into_diagnostic()?;
            if contents.len() > MAX_CACHE_BYTES {
                bail!("Python index cache for {name} exceeds {MAX_CACHE_BYTES} bytes");
            }
            serde_json::from_slice::<CachedIndex>(&contents).into_diagnostic()?
        } else {
            let response = self
                .client
                .get_limited_bytes_with_secure_auth_and_retry(
                    index_url.as_str(),
                    &self.auth,
                    Some("application/vnd.pypi.simple.v1+json"),
                    self.config.retry_opts(),
                    MAX_INDEX_BYTES,
                )
                .await
                .into_diagnostic()?;
            if response.body_truncated {
                bail!("Python index response for {name} exceeds {MAX_INDEX_BYTES} bytes");
            }
            if !response.status.is_success() {
                bail!("Python index request for {name} returned {}", response.status);
            }
            CachedIndex {
                url: response.url.parse().into_diagnostic()?,
                body: serde_json::from_slice(&response.body)
                    .into_diagnostic()
                    .wrap_err("Python index must support the Simple JSON API")?,
            }
        };
        if cached.body.get().len() > MAX_INDEX_BYTES {
            bail!("Python index response for {name} exceeds {MAX_INDEX_BYTES} bytes");
        }
        let candidates =
            candidates_from_page(cached.body.get(), &cached.url, name, &self.interpreter.target)?;
        if !self.config.offline {
            tokio::fs::create_dir_all(cache.parent().expect("cache file has a parent"))
                .await
                .into_diagnostic()?;
            let contents = serde_json::to_vec(&cached).into_diagnostic()?;
            if contents.len() > MAX_CACHE_BYTES {
                bail!("Python index cache for {name} exceeds {MAX_CACHE_BYTES} bytes");
            }
            pnpm_fs::write_atomic(&cache, &contents).into_diagnostic()?;
        }
        self.packages.candidates.insert(name.clone(), candidates);
        Ok(())
    }

    pub(super) async fn fetch_wheel<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        let wheel = self.download_wheel::<Reporter>(name, version).await?;
        self.remember(name.clone(), version.clone(), wheel);
        Ok(())
    }

    pub(super) async fn fetch_wheels<Reporter: self::Reporter + 'static>(
        &mut self,
        packages: &[LockedPackage],
    ) -> Result<()> {
        // The borrowed iterator needs a higher-ranked function pointer to keep preparation Send.
        let identity: fn(&LockedPackage) -> (PackageName, Version) =
            |package| (package.name.clone(), package.version.clone());
        let packages = packages.iter().map(identity);
        let registry = &*self;
        let results = stream::iter(packages.enumerate())
            .map(|(position, (name, version))| async move {
                let result = registry
                    .download_wheel::<Reporter>(&name, &version)
                    .await
                    .map(|wheel| ((name, version), wheel));
                (position, result)
            })
            .buffer_unordered(self.config.network_concurrency.clamp(1, 16))
            .collect::<BTreeMap<_, _>>()
            .await;
        for ((name, version), wheel) in results.into_values().collect::<Result<Vec<_>>>()? {
            self.remember(name, version, wheel);
        }
        Ok(())
    }

    /// Keep a downloaded wheel for both readers: the interpreter's full
    /// report for installing it, and the subset resolution reads.
    fn remember(&mut self, name: PackageName, version: Version, wheel: Wheel) {
        self.packages.metadata.insert(
            (name.clone(), version.clone()),
            pnpm_python_resolver::WheelMetadata {
                name: wheel.metadata.name.clone(),
                version: wheel.metadata.version.clone(),
                requires_dist: wheel.metadata.requires_dist.clone(),
                requires_python: wheel.metadata.requires_python.clone(),
                provides_extra: wheel.metadata.provides_extra.clone(),
            },
        );
        self.wheels.insert((name, version), wheel);
    }

    async fn download_wheel<Reporter: self::Reporter + 'static>(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> Result<Wheel> {
        let wheel = &self.packages.candidates[name][version].wheel;
        pnpm_python_resolver::validate_url(&Url::parse(&wheel.url).into_diagnostic()?)?;
        let Some((wheel_name, wheel_version, _)) =
            wheel_identity(&wheel.name, &self.interpreter.target.tags)?
        else {
            bail!("Python wheel is incompatible with this interpreter: {}", wheel.name)
        };
        if wheel_name != *name || wheel_version != *version {
            bail!("Python lockfile wheel identity mismatch: {}", wheel.name);
        }
        let integrity = wheel.integrity()?;
        let package_id = format!("python:{}", wheel.name);
        let files = IngestZipArchiveToStore {
            http_client: self.client,
            store_dir: &self.config.store_dir,
            store_index: self.store_index.clone(),
            store_index_writer: Some(Arc::clone(&self.writer)),
            verify_store_integrity: self.config.verify_store_integrity,
            strict_store_pkg_content_check: self.config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(&self.verified),
            package_integrity: &integrity,
            package_url: &wheel.url,
            package_id: &package_id,
            requester: "Python environment",
            prefetched_cas_paths: None,
            retry_opts: self.config.retry_opts(),
            auth_headers: &self.auth,
            archive_prefix: None,
            ignore_file_pattern: None,
            offline: self.config.offline,
            store_projection: ArchiveStoreProjection::RawArchive,
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .into_diagnostic()?;
        let files = files.into_iter().collect::<BTreeMap<_, _>>();
        let metadata: WheelMetadata = host::run(
            &self.interpreter.executable,
            "inspect",
            serde_json::json!({"files": files, "filename": wheel.name}),
        )
        .await?;
        if metadata.name.parse::<PackageName>().into_diagnostic()? != *name
            || metadata.version.parse::<Version>().into_diagnostic()? != *version
        {
            bail!("Python wheel metadata identity mismatch for {name}=={version}");
        }
        let (directory_name, directory_version) = metadata
            .dist_info
            .strip_suffix(".dist-info")
            .and_then(|stem| stem.rsplit_once('-'))
            .ok_or_else(|| {
                miette::miette!("invalid Python dist-info directory for {name}=={version}")
            })?;
        if directory_name.parse::<PackageName>().into_diagnostic()? != *name
            || directory_version.parse::<Version>().into_diagnostic()? != *version
        {
            bail!("Python dist-info directory identity mismatch for {name}=={version}");
        }
        Ok(Wheel { files, metadata })
    }
}
