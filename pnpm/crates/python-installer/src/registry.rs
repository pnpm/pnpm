use super::{Index, host, host::Interpreter, host::Wheel, host::WheelMetadata};
use futures_util::{StreamExt, stream};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_python_resolver::{Excluded, LockedWheel, Packages, Target, candidates_from_page};
use pnpm_reporter::Reporter;
use pnpm_tarball::{ArchiveStoreProjection, IngestZipArchiveToStore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = MAX_INDEX_BYTES + 64 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct CachedIndex {
    url: Url,
    body: Box<serde_json::value::RawValue>,
    #[serde(default)]
    missing: bool,
}

pub(super) struct Registry<'a> {
    pub(super) config: &'static Config,
    pub(super) client: &'a ThrottledClient,
    pub(super) index: &'a Index,
    pub(super) interpreter: &'a Interpreter,
    pub(super) store: pnpm_tarball::ArchiveStoreContext<'a>,
    pub(super) resolution: Resolution,
    /// What installing reads: the store paths of every wheel downloaded so
    /// far, beside the interpreter's full report on it.
    pub(super) wheels: BTreeMap<(PackageName, Version), Wheel>,
    pub(super) sources: super::sources::Sources<'a>,
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
    /// The distributions whose index page this run has downloaded. A
    /// project locking for several environments reads the page once and
    /// takes every later environment's candidates from the cache that
    /// download wrote.
    pub(super) downloaded: BTreeSet<PackageName>,
}

impl CachedIndex {
    fn from_response(
        name: &PackageName,
        response: &pnpm_network::SecureAuthResponse,
    ) -> Result<Self> {
        if response.status.as_u16() == 404 {
            return Ok(CachedIndex {
                url: response.url.parse().into_diagnostic()?,
                body: serde_json::value::RawValue::from_string(r#"{"files":[]}"#.to_string())
                    .into_diagnostic()?,
                missing: true,
            });
        }
        if response.body_truncated {
            bail!("Python index response for {name} exceeds {MAX_INDEX_BYTES} bytes");
        }
        if !response.status.is_success() {
            bail!("Python index request for {name} returned {}", response.status);
        }
        Ok(CachedIndex {
            missing: false,
            url: response.url.parse().into_diagnostic()?,
            body: serde_json::from_slice(&response.body)
                .into_diagnostic()
                .wrap_err("Python index must support the Simple JSON API")?,
        })
    }
}

impl Resolution {
    pub(super) fn new(target: Target) -> Self {
        Self { target, packages: Packages::new(), downloaded: BTreeSet::new() }
    }

    /// Answer for another environment, which takes its own candidates
    /// from the same index pages.
    pub(super) fn answer_for(&mut self, target: Target) {
        self.target = target;
        self.packages.excluded.clear();
        // Only what an index offers is taken per target. A project in this
        // repository is the same directory on every environment, so it
        // stays offered rather than being seeded again for each.
        self.packages.candidates.retain(|_, versions| {
            versions
                .values()
                .all(|candidate| candidate.directory().is_some())
        });
        self.packages.direct_urls.clear();
        self.packages.rejected_sources.clear();
    }

    /// Offer this environment the candidates an index page holds, keeping
    /// what the page held besides them for the failure that may name the
    /// distribution.
    fn offer(&mut self, name: &PackageName, page: &CachedIndex) -> Result<()> {
        let offered = candidates_from_page(page.body.get(), &page.url, name, &self.target)?;
        self.packages.candidates.insert(name.clone(), offered.candidates);
        self.packages.excluded.insert(name.clone(), offered.excluded);
        Ok(())
    }
}

impl Registry<'_> {
    pub(super) async fn fetch_index(&mut self, name: &PackageName) -> Result<()> {
        self.resolution.packages.candidates.insert(name.clone(), BTreeMap::new());
        self.resolution.packages.excluded.insert(name.clone(), Excluded::default());
        for index in self.index.extra_urls
            .iter()
            .chain(std::iter::once(&self.index.url))
        {
            let page = self.read_index(index, name).await?;
            if !page.missing {
                self.resolution.offer(name, &page)?;
                break;
            }
        }
        self.resolution.downloaded.insert(name.clone());
        if self.resolution.packages.candidates[name]
            .keys()
            .any(|version| {
                self.wheels
                    .get(&(name.clone(), version.clone()))
                    .is_some_and(|wheel| wheel.direct_url.is_some())
            })
        {
            self.resolution.packages.metadata.retain(|(distribution, _), _| distribution != name);
        }
        Ok(())
    }

    /// The Simple JSON index page for `name`: from the cache when this
    /// run already downloaded it, or an offline resolution is reading
    /// what an earlier run left behind, and from the index otherwise.
    async fn read_index(&self, index: &Url, name: &PackageName) -> Result<CachedIndex> {
        let index_url = index
            .join(&format!("{name}/"))
            .into_diagnostic()?;
        let cache = self.config.cache_dir
            .join("python-index-v3")
            .join(format!("{}.json", self.index.cache_key(&index_url)));
        let replayed = self.config.offline || self.resolution.downloaded.contains(name);
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
        CachedIndex::from_response(name, &response)
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

    /// Get the wheel every candidate holds that this run does not have
    /// already, which after a lockfile is seeded is what each locked
    /// package installs here: a download, or the build of a repository or
    /// a source distribution. Resolving another environment may have read
    /// a different build of the same version.
    pub(super) async fn fetch_wheels<Reporter: self::Reporter + 'static>(
        &mut self,
        requirements: &[pep508_rs::Requirement],
    ) -> Result<()> {
        // The stream owns what it walks: a borrowed iterator would have to be
        // `Send` for every lifetime to keep preparation `Send`.
        let mut wanted = Vec::new();
        for (name, versions) in &self.resolution.packages.candidates {
            for (version, candidate) in versions {
                // A project in this repository is built from its source,
                // so there is nothing to fetch for it.
                if candidate.wheel().is_none() {
                    continue;
                }
                if !self.has_wheel(name, version) {
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
        self.record_sources(&[])?;
        self.fetch_vcs::<Reporter>(requirements).await?;
        self.build_sdists::<Reporter>().await?;
        Ok(())
    }

    /// Keep a downloaded wheel for both readers: the interpreter's full
    /// report for installing it, and the subset resolution reads.
    pub(super) fn remember(&mut self, name: PackageName, version: Version, wheel: Wheel) {
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
        self.sources.fetched.insert(
            (name.clone(), version.clone()),
            self.resolution.packages.candidates[&name][&version].clone(),
        );
        self.wheels.insert((name, version), wheel);
    }

    pub(super) async fn download_wheel_from_buffer<Reporter: self::Reporter + 'static>(
        &self,
        name: &PackageName,
        version: &Version,
        buffer: Option<Vec<u8>>,
    ) -> Result<Wheel> {
        let wheel = self.resolution.packages.candidates[name][version]
            .wheel()
            .ok_or_else(|| miette::miette!("Python package {name} {version} is not a wheel"))?;
        validate_wheel_identity(wheel, &self.resolution.target.tags, name, version)?;
        let files = self.ingest_wheel::<Reporter>(wheel, buffer).await?;
        let files: BTreeMap<_, _> = files.into_iter().collect();
        let metadata = host::inspect(&self.interpreter.executable, &files).await?;
        validate_wheel_metadata(&metadata, name, version)?;
        Ok(Wheel {
            filename: wheel.name.clone(),
            files,
            metadata,
            direct_url: self.source_provenance(name),
        })
    }
    async fn ingest_wheel<Reporter: self::Reporter + 'static>(
        &self,
        wheel: &LockedWheel,
        buffer: Option<Vec<u8>>,
    ) -> Result<std::collections::HashMap<String, std::path::PathBuf>> {
        let integrity = wheel.integrity()?;
        let package_id = format!("python:{}", wheel.name);
        let ingestion = IngestZipArchiveToStore {
            fetching: pnpm_tarball::ArchiveFetchOptions {
                http_client: self.client,
                auth_headers: &self.index.auth,
                retry_opts: self.config.retry_opts(),
                offline: self.config.offline,
            },
            package: pnpm_tarball::ZipArchivePackage {
                max_bytes: Some(super::sources::MAX_WHEEL_BYTES),
                integrity: &integrity,
                url: &wheel.url,
                id: &package_id,
            },
            store: self.store.clone(),

            requester: "Python environment",

            archive_prefix: None,
            ignore_file_pattern: None,

            store_projection: ArchiveStoreProjection::RawArchive,
        };
        match buffer {
            Some(buffer) => ingestion.run_with_buffer::<Reporter>(buffer).await,
            None => ingestion.run_without_mem_cache::<Reporter>().await,
        }
        .into_diagnostic()
    }

    async fn download_wheel<Reporter: self::Reporter + 'static>(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> Result<Wheel> {
        self.download_wheel_from_buffer::<Reporter>(name, version, None).await
    }
}

/// The index cached from an earlier run, which is the only source an
/// offline resolution has.
async fn read_cached_index(cache: &std::path::Path, name: &PackageName) -> Result<CachedIndex> {
    super::cache::read_json(cache, MAX_CACHE_BYTES, &format!("Python index cache for {name}"))
        .await
        .wrap_err_with(|| format!("Python index for {name} is not cached for offline resolution"))
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

#[cfg(test)]
mod tests;
