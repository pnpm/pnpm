use super::{
    CustomFetchOutcome,
    CustomFetcherSession,
    FetchedTarball,
    InstallPackageBySnapshotError,
    LockfileResolution,
    ResolvedTarballMetadata,
    decode_resolution,
    fetch_custom_tarball,
};
use pnpm_reporter::Reporter;
use pnpm_tarball::IngestTarballToStore;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
};

impl CustomFetcherSession {
    pub async fn resolve_tarball_metadata<Reporter: self::Reporter>(
        &self,
        download: IngestTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
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
                fetch_custom_tarball::<Reporter>(download.clone(), source, &lockfile_dir).await?
            else {
                return Ok(ResolvedTarballMetadata { resolution, manifest: None });
            };
            tarball
        };
        let metadata =
            resolve_archive_metadata((&resolution, source), &tarball, download.package.id).await?;
        self.cache_resolved_tarball(download.package.id, (&resolution, source), tarball);
        Ok(metadata)
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
        self.completed
            .lock()
            .unwrap()
            .insert((package_id.to_owned(), tarball.integrity.to_string()), tarball);
    }
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
