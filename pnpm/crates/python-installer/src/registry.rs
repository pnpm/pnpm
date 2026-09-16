use super::{Index, host, host::Interpreter, host::Wheel, host::WheelMetadata};
use futures_util::{StreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_python_resolver::{Packages, Target, candidates_from_page};
use pnpm_reporter::Reporter;
use pnpm_tarball::{ArchiveStoreProjection, IngestZipArchiveToStore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tokio::io::AsyncReadExt;
use url::Url;

const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = MAX_INDEX_BYTES + 64 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct CachedIndex {
    url: Url,
    body: Box<serde_json::value::RawValue>,
}

pub(super) struct Registry<'a> {
    pub(super) config: &'static Config,
    pub(super) client: &'a ThrottledClient,
    pub(super) index: &'a Index,
    pub(super) interpreter: &'a Interpreter,
    pub(super) store: pnpm_tarball::ArchiveStoreContext<'a>,
    pub(super) resolution: Resolution,
    /// The distributions whose index page this run has downloaded. A
    /// project locking for several environments reads the page once and
    /// takes every later environment's candidates from the cache that
    /// download wrote.
    pub(super) downloaded: BTreeSet<PackageName>,
    /// What installing reads: the store paths of every wheel downloaded so
    /// far, beside the interpreter's full report on it.
    pub(super) wheels: BTreeMap<(PackageName, Version), Wheel>,
}

/// What a resolution reads and what it is answering for: the environment
/// being resolved, and the candidates and metadata gathered for it.
pub(super) struct Resolution {
    /// The environment the registry is answering for: the one being
    /// resolved while a lockfile is written, and the interpreter running
    /// the install while its environment is built.
    pub(super) target: Target,
    /// The candidates each distribution offers this environment, and the
    /// metadata of the wheels the resolution has looked at.
    pub(super) packages: Packages,
}

impl Resolution {
    pub(super) fn new(target: Target) -> Self {
        Self { target, packages: Packages::new() }
    }

    /// Answer for another environment, which takes its own candidates
    /// from the same index pages.
    pub(super) fn answer_for(&mut self, target: Target) {
        self.target = target;
        self.packages.candidates.clear();
    }

    /// Offer this environment the candidates an index page holds.
    fn offer(&mut self, name: &PackageName, page: &CachedIndex) -> Result<()> {
        let candidates = candidates_from_page(page.body.get(), &page.url, name, &self.target)?;
        self.packages.candidates.insert(name.clone(), candidates);
        Ok(())
    }
}

impl Registry<'_> {
    pub(super) async fn fetch_index(&mut self, name: &PackageName) -> Result<()> {
        let page = self.read_index(name).await?;
        self.downloaded.insert(name.clone());
        self.resolution.offer(name, &page)
    }

    /// The Simple JSON index page for `name`: from the cache when this
    /// run already downloaded it, or an offline resolution is reading
    /// what an earlier run left behind, and from the index otherwise.
    async fn read_index(&self, name: &PackageName) -> Result<CachedIndex> {
        let index_url = self.index.url
            .join(&format!("{name}/"))
            .into_diagnostic()?;
        let cache = self.config.cache_dir
            .join("python-index-v2")
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(index_url.as_str())));
        let replayed = self.config.offline || self.downloaded.contains(name);
        let cached = if replayed {
            read_cached_index(&cache, name).await?
        } else {
            self.download_index(&index_url, name).await?
        };
        if cached.body.get().len() > MAX_INDEX_BYTES {
            bail!("Python index response for {name} exceeds {MAX_INDEX_BYTES} bytes");
        }
        if !replayed {
            tokio::fs::create_dir_all(cache.parent().expect("cache file has a parent"))
                .await
                .into_diagnostic()?;
            let contents = serde_json::to_vec(&cached).into_diagnostic()?;
            if contents.len() > MAX_CACHE_BYTES {
                bail!("Python index cache for {name} exceeds {MAX_CACHE_BYTES} bytes");
            }
            pnpm_fs::write_atomic(&cache, &contents).into_diagnostic()?;
        }
        Ok(cached)
    }

    /// Fetch the Simple JSON index for `name` from the configured index.
    async fn download_index(&self, index_url: &Url, name: &PackageName) -> Result<CachedIndex> {
        let response = self.client
            .get_limited_bytes_with_secure_auth_and_retry(
                index_url.as_str(),
                &self.index.auth,
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
        Ok(CachedIndex {
            url: response.url.parse().into_diagnostic()?,
            body: serde_json::from_slice(&response.body)
                .into_diagnostic()
                .wrap_err("Python index must support the Simple JSON API")?,
        })
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

    /// Download the wheel every candidate holds that is not downloaded
    /// already, which after a lockfile is seeded is the wheel each locked
    /// package installs here. Resolving another environment may have read
    /// a different build of the same version.
    pub(super) async fn fetch_wheels<Reporter: self::Reporter + 'static>(&mut self) -> Result<()> {
        // The stream owns what it walks: a borrowed iterator would have to be
        // `Send` for every lifetime to keep preparation `Send`.
        let mut wanted = Vec::new();
        for (name, versions) in &self.resolution.packages.candidates {
            for (version, candidate) in versions {
                let downloaded = self.wheels.get(&(name.clone(), version.clone()));
                if downloaded.is_none_or(|wheel| {
                    wheel.filename != candidate.wheel.name
                }) {
                    wanted.push((name.clone(), version.clone()));
                }
            }
        }
        let registry = &*self;
        let results = stream::iter(wanted.into_iter().enumerate())
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
        self.resolution.packages.metadata.insert(
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
        let wheel = &self.resolution.packages.candidates[name][version].wheel;
        validate_wheel_identity(wheel, &self.resolution.target.tags, name, version)?;
        let integrity = wheel.integrity()?;
        let package_id = format!("python:{}", wheel.name);
        let files = IngestZipArchiveToStore {
            fetching: pnpm_tarball::ArchiveFetchOptions {
                http_client: self.client,
                auth_headers: &self.index.auth,
                retry_opts: self.config.retry_opts(),
                offline: self.config.offline,
            },
            package: pnpm_tarball::ZipArchivePackage {
                integrity: &integrity,
                url: &wheel.url,
                id: &package_id,
            },
            store: self.store.clone(),

            requester: "Python environment",

            archive_prefix: None,
            ignore_file_pattern: None,

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
        validate_wheel_metadata(&metadata, name, version)?;
        Ok(Wheel { filename: wheel.name.clone(), files, metadata })
    }
}

/// The index cached from an earlier run, which is the only source an
/// offline resolution has.
async fn read_cached_index(cache: &std::path::Path, name: &PackageName) -> Result<CachedIndex> {
    let file = tokio::fs::File::open(cache).await
        .into_diagnostic()
        .wrap_err_with(|| {
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
    serde_json::from_slice::<CachedIndex>(&contents).into_diagnostic()
}

fn validate_wheel_metadata(
    metadata: &WheelMetadata,
    name: &PackageName,
    version: &Version,
) -> Result<()> {
    if metadata.name.parse::<PackageName>().into_diagnostic()? != *name
        || metadata.version.parse::<Version>().into_diagnostic()? != *version
    {
        bail!("Python wheel metadata identity mismatch for {name}=={version}");
    }
    let (directory_name, directory_version) = metadata.dist_info
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
    Ok(())
}

fn validate_wheel_identity(
    wheel: &pnpm_python_resolver::LockedWheel,
    tags: &[String],
    name: &PackageName,
    version: &Version,
) -> Result<()> {
    pnpm_python_resolver::validate_url(&Url::parse(&wheel.url).into_diagnostic()?)?;
    wheel.check_installable(tags, name, version)
}
