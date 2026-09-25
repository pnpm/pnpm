use super::{
    CustomFetchOutcome, CustomFetcherSession, FetchedTarball, InstallPackageBySnapshotError,
    LockfileResolution, ResolvedTarballMetadata, decode_resolution, fetch_custom_tarball,
    fetch_identity,
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
                return Ok(ResolvedTarballMetadata {
                    resolution: recorded_resolution(original, resolution),
                    manifest: None,
                });
            };
            tarball
        };
        let mut metadata =
            resolve_archive_metadata((&resolution, source), &tarball, download.package.id).await?;
        metadata.resolution = recorded_resolution(original, metadata.resolution);
        // The install pass lays a reused fetch out by the recorded resolution,
        // so a fetch installed from elsewhere in its archive cannot be reused.
        if git_hosted_subdir(&metadata.resolution) == git_hosted_subdir(source) {
            self.record_completed_identities(download.package.id, &metadata, tarball);
        }
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
        metadata: &ResolvedTarballMetadata,
        tarball: Arc<FetchedTarball>,
    ) {
        let Some(identity) = fetch_identity(&metadata.resolution, Some(&tarball.integrity)) else {
            return;
        };
        self.cache_resolved_tarball(package_id, &identity, Arc::clone(&tarball));
        let Some(manifest) = &metadata.manifest else { return };
        let (Some(name), Some(version)) = (
            manifest.get("name").and_then(Value::as_str),
            manifest.get("version").and_then(Value::as_str),
        ) else {
            return;
        };
        let name_ver = format!("{name}@{version}");
        if name_ver != package_id {
            self.cache_resolved_tarball(&name_ver, &identity, tarball);
        }
    }

    fn cache_resolved_tarball(
        &self,
        package_id: &str,
        identity: &str,
        tarball: Arc<FetchedTarball>,
    ) {
        self.completed
            .lock()
            .unwrap()
            .insert((package_id.to_owned(), identity.to_owned()), tarball);
    }
}

/// The resolution the lockfile records: a custom one as its resolver wrote it,
/// since the fetch only interprets it, and otherwise the fetcher's.
fn recorded_resolution(
    original: &LockfileResolution,
    fetched: LockfileResolution,
) -> LockfileResolution {
    // The fetcher's copy of a custom resolution carries the scratch fields a
    // `canFetch` left on it, which `decode_resolution` strips only from
    // resolutions that have no `type`.
    if matches!(original, LockfileResolution::Custom(_)) { original.clone() } else { fetched }
}

/// Where in a git-hosted archive the package is installed from, `Some(None)`
/// being the archive root. `None` for a resolution whose archive installs as is.
fn git_hosted_subdir(resolution: &LockfileResolution) -> Option<Option<&str>> {
    match resolution {
        LockfileResolution::Tarball(tarball) if tarball.is_git_hosted() => {
            Some(tarball.path.as_deref())
        }
        _ => None,
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
