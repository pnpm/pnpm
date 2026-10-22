mod callbacks;
mod metadata;

use crate::{
    InstallPackageBySnapshotError, install_package_by_snapshot::local_file_tarball_install_url,
};
use callbacks::{NativeCallbacks, Verified, WalkedDirectory, drive_fetcher};
use pnpm_config::Config;
use pnpm_hooks::{CustomFetcher, custom_fetcher_adapter::CustomFetcherPicker};
use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_reporter::Reporter;
use pnpm_tarball::{FetchedTarball, IngestTarballToStore, TarballError};
use serde::Deserialize;
use serde_json::Value;
use ssri::Integrity;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(crate) enum CustomFetchOutcome {
    Declined(LockfileResolution),
    Delegate { resolution: LockfileResolution, delegate: LockfileResolution },
    Fetched { resolution: LockfileResolution, tarball: Arc<FetchedTarball> },
}

/// What a resolve-time archive fetch settles for a tarball-shaped
/// resolution: the resolution the lockfile records and the manifest the
/// dependency walk reads the package's children from.
#[derive(Debug, Clone)]
pub struct ResolvedTarballMetadata {
    pub resolution: LockfileResolution,
    /// `None` when no archive was fetched, or when the archive ships no
    /// readable `package.json`.
    pub manifest: Option<Arc<Value>>,
}

/// Shares verified custom fetches between fresh resolution and materialization.
pub struct CustomFetcherSession {
    picker: CustomFetcherPicker,
    completed: Mutex<HashMap<(String, String), Arc<FetchedTarball>>>,
    /// How the `fetchers.directory` callback lists a directory. Defaults to
    /// `true`, the install pass's `!deployAllFiles`.
    include_only_package_files: bool,
}

impl CustomFetcherSession {
    #[must_use]
    pub fn new(fetchers: Vec<Arc<dyn CustomFetcher>>) -> Self {
        Self {
            picker: CustomFetcherPicker::new(fetchers),
            completed: Mutex::new(HashMap::new()),
            include_only_package_files: true,
        }
    }

    /// A session whose `fetchers.directory` lists a directory as the
    /// install pass does under `config`.
    #[must_use]
    pub fn with_config(fetchers: Vec<Arc<dyn CustomFetcher>>, config: &Config) -> Self {
        Self { include_only_package_files: !config.deploy_all_files, ..Self::new(fetchers) }
    }

    pub(crate) async fn fetch<Reporter: self::Reporter>(
        &self,
        download: IngestTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
    ) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
        let package_id = download.package.id;
        let locked = original.checkable_integrity();
        let download = IngestTarballToStore {
            package: pnpm_tarball::TarballPackage { integrity: locked, ..download.package },
            ..download
        };
        if let Some(tarball) = self.completed_fetch(package_id, original, locked) {
            return Ok(CustomFetchOutcome::Fetched { resolution: original.clone(), tarball });
        }
        let selection = self.picker
            .pick_fetcher(package_id, &serde_json::json!(original))
            .await
            .map_err(|error| failure(package_id, error))?;
        let Some(fetcher) = selection.fetcher else {
            return decode_resolution(selection.resolution, locked, package_id)
                .map(CustomFetchOutcome::Declined);
        };
        let selected_resolution = selection.resolution;
        let lockfile_dir = PathBuf::from(
            opts.get("lockfileDir")
                .and_then(Value::as_str)
                .unwrap_or(download.requester),
        );
        let (result, verified) = drive_fetcher::<Reporter>(
            fetcher,
            &NativeCallbacks {
                download: &download,
                lockfile_dir: &lockfile_dir,
                include_only_package_files: self.include_only_package_files,
            },
            selected_resolution.clone(),
            opts,
        )
        .await?;
        decode_fetch_outcome(result, verified, selected_resolution, locked, package_id)
    }

    fn completed_fetch(
        &self,
        package_id: &str,
        resolution: &LockfileResolution,
        integrity: Option<&Integrity>,
    ) -> Option<Arc<FetchedTarball>> {
        let identity = fetch_identity(resolution, integrity)?;
        self.completed
            .lock()
            .unwrap()
            .get(&(package_id.to_owned(), identity))
            .cloned()
    }
}

/// What a completed fetch of `resolution` is filed under besides its package
/// id: the integrity its archive was checked against, or for a custom
/// resolution, which carries none pacquet can check, the resolution itself.
fn fetch_identity(
    resolution: &LockfileResolution,
    integrity: Option<&Integrity>,
) -> Option<String> {
    match resolution {
        LockfileResolution::Custom(_) => serde_json::to_string(resolution).ok(),
        _ => integrity.map(ToString::to_string),
    }
}

fn decode_fetch_outcome(
    result: Value,
    verified: Verified,
    selected_resolution: Value,
    locked: Option<&Integrity>,
    package_id: &str,
) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
    if result.get("filesMap").is_some() {
        let returned: ReturnedFiles =
            serde_json::from_value(result).map_err(|error| failure(package_id, error))?;
        if returned.local {
            // The install pass walks the directory again, as it does for a
            // `{ delegate }` envelope naming it.
            let directory = walked_directory(&returned, verified.directories, package_id)?;
            return Ok(CustomFetchOutcome::Delegate {
                resolution: decode_resolution(selected_resolution, None, package_id)?,
                delegate: LockfileResolution::Directory(DirectoryResolution { directory }),
            });
        }
        let tarball = verified_tarball(&returned, verified.archives, package_id)?;
        let resolution = decode_resolution(selected_resolution, None, package_id)?;
        return Ok(CustomFetchOutcome::Fetched { resolution, tarball });
    }
    let Some(delegate) = result.get("delegate") else {
        return Err(failure(
            package_id,
            "unhandled response: expected a delegate or native fetched files",
        ));
    };
    if !["type", "tarball", "integrity"]
        .iter()
        .any(|key| delegate.get(key).is_some())
    {
        return Err(failure(package_id, "invalid delegate resolution"));
    }
    Ok(CustomFetchOutcome::Delegate {
        resolution: decode_resolution(selected_resolution, None, package_id)?,
        delegate: decode_resolution(delegate.clone(), locked, package_id)
            .map_err(|error| match error {
                InstallPackageBySnapshotError::CustomFetcher(message) => {
                    InstallPackageBySnapshotError::CustomFetcher(format!(
                        "invalid delegate resolution: {message}",
                    ))
                }
                error => error,
            })?,
    })
}

/// The archive pacquet hashed itself that carries the files the hook
/// returned. A hook may only hand back files a native fetch verified,
/// and every verified archive matching those files must agree on the
/// integrity, or the fetch is ambiguous.
fn verified_tarball(
    returned: &ReturnedFiles,
    verified: Vec<Arc<FetchedTarball>>,
    package_id: &str,
) -> Result<Arc<FetchedTarball>, InstallPackageBySnapshotError> {
    let mut matches = verified
        .into_iter()
        .filter(|verified: &Arc<FetchedTarball>| {
            verified.files_map == returned.files_map
                && returned.integrity
                    .as_ref()
                    .is_none_or(|integrity| integrity == &verified.integrity.to_string())
        });
    let tarball = matches
        .next()
        .ok_or_else(|| {
            failure(
                package_id,
                "custom fetcher returned files not verified by a native tarball fetcher",
            )
        })?;
    if matches.any(|other| other.integrity != tarball.integrity) {
        return Err(failure(package_id, "custom fetcher returned an ambiguous archive integrity"));
    }
    Ok(tarball)
}

/// The directory a native directory walk listed with exactly the files the
/// hook returned.
fn walked_directory(
    returned: &ReturnedFiles,
    walked: Vec<WalkedDirectory>,
    package_id: &str,
) -> Result<String, InstallPackageBySnapshotError> {
    let mut matches = walked
        .into_iter()
        .filter(|walked| walked.files_map == returned.files_map);
    let directory = matches
        .next()
        .ok_or_else(|| {
            failure(
                package_id,
                "custom fetcher returned local files not listed by the native directory fetcher",
            )
        })?
        .directory;
    if matches.any(|other| other.directory != directory) {
        return Err(failure(package_id, "custom fetcher returned an ambiguous directory"));
    }
    Ok(directory)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedFiles {
    files_map: HashMap<String, PathBuf>,
    integrity: Option<String>,
    #[serde(default)]
    local: bool,
}

#[derive(Deserialize)]
struct TarballLocation {
    tarball: String,
    integrity: Option<Integrity>,
}

fn failure(package_id: &str, error: impl std::fmt::Display) -> InstallPackageBySnapshotError {
    InstallPackageBySnapshotError::CustomFetcher(format!("{package_id}: {error}"))
}

fn decode_resolution(
    mut value: Value,
    locked: Option<&Integrity>,
    package_id: &str,
) -> Result<LockfileResolution, InstallPackageBySnapshotError> {
    if let Some(integrity) = locked {
        if !value
            .get("type")
            .is_none_or(|kind| kind.is_null() || kind == "binary")
        {
            return Err(InstallPackageBySnapshotError::CustomFetcherIntegrityMismatch {
                package_id: package_id.to_owned(),
            });
        }
        let object =
            value.as_object_mut().ok_or_else(|| failure(package_id, "invalid resolution"))?;
        object.insert("integrity".to_owned(), serde_json::json!(integrity.to_string()));
    }
    // Hook-local fields may pass between canFetch calls, but are not lockfile fields.
    if value.get("type").is_none_or(Value::is_null)
        && let Some(object) = value.as_object_mut()
    {
        object.retain(|key, _| {
            matches!(key.as_str(), "tarball" | "integrity" | "gitHosted" | "path")
        });
    }
    serde_json::from_value(value).map_err(|error| failure(package_id, error))
}

/// `None` when the hook pointed the package at a source that carries no archive
/// digest — a directory or a git checkout — or at a registry resolution while
/// the caller has no URL for it. Only a fresh install's missing-metadata
/// discovery calls this, so there is nothing to hash and nothing to verify; the
/// install pass materializes such a resolution through its own dispatch.
async fn fetch_custom_tarball<Reporter: self::Reporter>(
    download: IngestTarballToStore<'_>,
    resolution: &LockfileResolution,
    lockfile_dir: &Path,
) -> Result<Option<Arc<FetchedTarball>>, InstallPackageBySnapshotError> {
    let location = match resolution {
        LockfileResolution::Tarball(resolution) => TarballLocation {
            tarball: resolution.tarball.clone(),
            integrity: resolution.integrity.clone(),
        },
        LockfileResolution::Registry(resolution) if !download.package.url.is_empty() => {
            return load_or_fetch_registry_tarball::<Reporter>(download, &resolution.integrity)
                .await
                .map_err(InstallPackageBySnapshotError::DownloadTarball)
                .map(Some);
        }
        _ => return Ok(None),
    };
    fetch_location::<Reporter>(&download, location, lockfile_dir).await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
        .map(Some)
}

/// A registry resolution always pins its integrity, so its archive is the one
/// the store already holds under that hash whenever an earlier install fetched
/// it. Reusing that copy keeps a warm install off the network and lets an
/// offline one read the manifest.
async fn load_or_fetch_registry_tarball<Reporter: self::Reporter>(
    download: IngestTarballToStore<'_>,
    integrity: &Integrity,
) -> Result<Arc<FetchedTarball>, TarballError> {
    let files_map = IngestTarballToStore {
        package: pnpm_tarball::TarballPackage { integrity: Some(integrity), ..download.package },
        ..download
    }
    .run_without_mem_cache::<Reporter>()
    .await?;
    Ok(Arc::new(FetchedTarball {
        integrity: integrity.clone(),
        tar_integrity: None,
        manifest: pnpm_tarball::read_cas_package_json(&files_map, "package.json").await?,
        requires_build: crate::requires_build_from_cas_paths(&files_map),
        files_map,
        bins: None,
    }))
}

async fn fetch_location<Reporter: self::Reporter>(
    download: &IngestTarballToStore<'_>,
    location: TarballLocation,
    lockfile_dir: &Path,
) -> Result<Arc<FetchedTarball>, TarballError> {
    let url = local_file_tarball_install_url(location.tarball.as_str().into(), lockfile_dir);
    IngestTarballToStore {
        package: pnpm_tarball::TarballPackage {
            integrity: download.package.integrity.or_else(|| {
                location.integrity
                    .as_ref()
                    .filter(|value| !value.hashes.is_empty())
            }),
            url: &url,
            ..download.clone().package
        },

        ..download.clone()
    }
    .fetch_and_extract::<Reporter>()
    .await
    .map(Arc::new)
}
