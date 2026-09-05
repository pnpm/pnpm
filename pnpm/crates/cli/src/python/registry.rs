use super::{
    host::{self, Interpreter, Wheel, WheelMetadata},
    lockfile::LockedWheel,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::PackageName;
use pnpm_config::Config;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{ArchiveStoreProjection, IngestZipArchiveToStore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use url::Url;

#[derive(Deserialize)]
struct SimpleIndex {
    files: Vec<IndexFile>,
}

#[derive(Serialize, Deserialize)]
struct CachedIndex {
    url: Url,
    body: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexFile {
    filename: String,
    url: String,
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    yanked: serde_json::Value,
    requires_python: Option<String>,
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
    pub(super) candidates: BTreeMap<PackageName, BTreeMap<Version, LockedWheel>>,
    pub(super) wheels: BTreeMap<(PackageName, Version), Wheel>,
}

impl Registry<'_> {
    pub(super) async fn fetch_index(&mut self, name: &PackageName) -> Result<()> {
        let index_url = self.index.join(&format!("{name}/")).into_diagnostic()?;
        let cache = self
            .config
            .cache_dir
            .join("python-index")
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(index_url.as_str())));
        let cached = if self.config.offline {
            let contents = tokio::fs::read(&cache).await.into_diagnostic().wrap_err_with(|| {
                format!("Python index for {name} is not cached for offline resolution")
            })?;
            serde_json::from_slice::<CachedIndex>(&contents).into_diagnostic()?
        } else {
            let response = self
                .client
                .get_bytes_with_secure_auth_and_retry(
                    index_url.as_str(),
                    &self.auth,
                    Some("application/vnd.pypi.simple.v1+json"),
                    self.config.retry_opts(),
                )
                .await
                .into_diagnostic()?;
            if !response.status.is_success() {
                bail!("Python index request for {name} returned {}", response.status);
            }
            CachedIndex { url: response.url.parse().into_diagnostic()?, body: response.body }
        };
        let index: SimpleIndex = serde_json::from_slice(&cached.body)
            .into_diagnostic()
            .wrap_err("Python index must support the Simple JSON API")?;
        let mut candidates = BTreeMap::<Version, (usize, LockedWheel)>::new();
        for file in index.files {
            if !matches!(file.yanked, serde_json::Value::Null | serde_json::Value::Bool(false)) {
                continue;
            }
            let Some((wheel_name, version, rank)) =
                wheel_identity(&file.filename, self.interpreter)?
            else {
                continue;
            };
            if wheel_name != *name {
                bail!("Python index for {name} contains a wheel for {wheel_name}");
            }
            if let Some(requirement) = &file.requires_python {
                let specifiers: VersionSpecifiers = requirement.parse().into_diagnostic()?;
                if !specifiers.contains(self.interpreter.environment.python_full_version()) {
                    continue;
                }
            }
            let url = cached.url.join(&file.url).into_diagnostic()?;
            validate_url(&url)?;
            let wheel =
                LockedWheel { name: file.filename, url: url.to_string(), hashes: file.hashes };
            wheel.integrity()?;
            if candidates.get(&version).is_none_or(|(previous, existing)| {
                (rank, &wheel.name) < (*previous, &existing.name)
            }) {
                candidates.insert(version, (rank, wheel));
            }
        }
        if !self.config.offline {
            tokio::fs::create_dir_all(cache.parent().expect("cache file has a parent"))
                .await
                .into_diagnostic()?;
            pnpm_fs::write_atomic(&cache, &serde_json::to_vec(&cached).into_diagnostic()?)
                .into_diagnostic()?;
        }
        self.candidates.insert(
            name.clone(),
            candidates.into_iter().map(|(version, (_, wheel))| (version, wheel)).collect(),
        );
        Ok(())
    }

    pub(super) async fn fetch_wheel<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        let wheel = &self.candidates[name][version];
        validate_url(&Url::parse(&wheel.url).into_diagnostic()?)?;
        let Some((wheel_name, wheel_version, _)) = wheel_identity(&wheel.name, self.interpreter)?
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
        let metadata: WheelMetadata =
            host::run(&self.interpreter.executable, "inspect", serde_json::json!({"files": files}))
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
        self.wheels.insert((name.clone(), version.clone()), Wheel { files, metadata });
        Ok(())
    }
}

pub(super) fn wheel_identity(
    filename: &str,
    interpreter: &Interpreter,
) -> Result<Option<(PackageName, Version, usize)>> {
    let Some(stem) = filename.strip_suffix(".whl") else { return Ok(None) };
    let parts = stem.split('-').collect::<Vec<_>>();
    if !(parts.len() == 5 || parts.len() == 6) || filename.contains(['/', '\\']) {
        bail!("invalid Python wheel filename: {filename}");
    }
    let tags = &parts[parts.len() - 3..];
    let rank = interpreter.tags.iter().position(|tag| {
        let actual = tag.split('-').collect::<Vec<_>>();
        actual.len() == 3
            && tags.iter().zip(actual).all(|(supported, actual)| {
                supported.split('.').any(|supported| supported == actual)
            })
    });
    rank.map(|rank| {
        Ok((parts[0].parse().into_diagnostic()?, parts[1].parse().into_diagnostic()?, rank))
    })
    .transpose()
}

pub(super) fn validate_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Python artifacts require HTTP(S) URLs without embedded credentials");
    }
    Ok(())
}
