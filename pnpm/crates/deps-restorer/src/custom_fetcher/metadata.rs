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
    let commit_addressed = matches!(
        resolution,
        LockfileResolution::Tarball(resolution)
            if pnpm_lockfile::is_git_hosted_tarball_url(&resolution.tarball),
    );
    let resolution = if commit_addressed {
        resolution.clone()
    } else {
        decode_resolution(serde_json::json!(resolution), Some(&tarball.integrity), package_id)?
    };
    Ok(ResolvedTarballMetadata { resolution, manifest })
}

#[cfg(test)]
mod tests;

