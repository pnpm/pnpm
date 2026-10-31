mod callbacks;
mod metadata;
mod outcomes;

use crate::{
    InstallPackageBySnapshotError, install_package_by_snapshot::local_file_tarball_install_url,
};
use callbacks::{NativeCallbacks, NativeFetcherResult, Verified, WalkedDirectory, drive_fetcher};
use outcomes::{decode_fetch_outcome, decode_resolution};
use pnpm_config::Config;
use pnpm_directory_fetcher::CopyFiles;
use pnpm_hooks::{CustomFetcher, custom_fetcher_adapter::CustomFetcherPicker};
use pnpm_lockfile::LockfileResolution;
use pnpm_reporter::Reporter;
use pnpm_tarball::{FetchedTarball, IngestTarballToStore, TarballError};
use serde_json::Value;
use ssri::Integrity;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(serde::Deserialize)]
struct TarballLocation {
    tarball: String,
    integrity: Option<Integrity>,
}

fn failure(package_id: &str, error: impl std::fmt::Display) -> InstallPackageBySnapshotError {
    InstallPackageBySnapshotError::CustomFetcher(format!("{package_id}: {error}"))
}

pub(crate) enum CustomFetchOutcome {
    Declined(LockfileResolution),
    Delegate {
        resolution: LockfileResolution,
        delegate: LockfileResolution,
    },
    Fetched {
        resolution: LockfileResolution,
        tarball: Arc<FetchedTarball>,
    },
    FetchedFiles {
        resolution: LockfileResolution,
        package: NativeFetcherResult,
    },
    /// The hook returned the result of `fetchers.directory` unchanged.
    Walked {
        resolution: LockfileResolution,
        directory: WalkedDirectory,
    },
}

enum CompletedFetch {
    Tarball(Arc<FetchedTarball>),
    Package(NativeFetcherResult),
}

/// What a resolve-time archive fetch settles for a tarball-shaped
/// resolution: the resolution the lockfile records and the manifest the
/// dependency walk reads the package's children from.
#[derive(Debug, Clone)]
pub struct ResolvedTarballMetadata {
    pub resolution: LockfileResolution,
    /// `None` when the fetch produced neither an archive nor a
    /// `fetchers.directory` walk, or when what it produced has no readable
    /// `package.json`.
    pub manifest: Option<Arc<Value>>,
}

/// Shares verified custom fetches between fresh resolution and materialization.
pub struct CustomFetcherSession {
    picker: CustomFetcherPicker,
    completed: Mutex<HashMap<(String, String), CompletedFetch>>,
    settings: Option<callbacks::NativeFetcherSettings>,
    git_source_cache: pnpm_git_fetcher::GitSourceCache,
    files: CopyFiles,
}

impl CustomFetcherSession {
    #[must_use]
    pub fn new(fetchers: Vec<Arc<dyn CustomFetcher>>) -> Self {
        Self {
            picker: CustomFetcherPicker::new(fetchers),
            completed: Mutex::new(HashMap::new()),
            settings: None,
            git_source_cache: pnpm_git_fetcher::GitSourceCache::default(),
            files: CopyFiles::default(),
        }
    }

    /// A session whose `fetchers.directory` lists a directory as the
    /// install pass does under `config`.
    pub fn with_config(
        fetchers: Vec<Arc<dyn CustomFetcher>>,
        config: &Config,
    ) -> Result<Self, crate::VersionPolicyError> {
        Ok(Self {
            files: CopyFiles::from_deploy_all_files(config.deploy_all_files),
            settings: Some(callbacks::NativeFetcherSettings::from_config(config)?),
            ..Self::new(fetchers)
        })
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
        if let Some(completed) = self.completed_fetch(package_id, original, locked) {
            return Ok(match completed {
                CompletedFetch::Tarball(tarball) => {
                    CustomFetchOutcome::Fetched { resolution: original.clone(), tarball }
                }
                CompletedFetch::Package(package) => {
                    CustomFetchOutcome::FetchedFiles { resolution: original.clone(), package }
                }
            });
        }
        self.fetch_uncached::<Reporter>(&download, original, locked, package_id, opts).await
    }

    async fn fetch_uncached<Reporter: self::Reporter>(
        &self,
        download: &IngestTarballToStore<'_>,
        original: &LockfileResolution,
        locked: Option<&Integrity>,
        package_id: &str,
        opts: Value,
    ) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
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
                download,
                lockfile_dir: &lockfile_dir,
                files: self.files,
                settings: self.settings.as_ref(),
                git_source_cache: &self.git_source_cache,
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
    ) -> Option<CompletedFetch> {
        let identity = fetch_identity(resolution, integrity)?;
        self.completed
            .lock()
            .unwrap()
            .get(&(package_id.to_owned(), identity))
            .map(|fetch| match fetch {
                CompletedFetch::Tarball(tarball) => CompletedFetch::Tarball(Arc::clone(tarball)),
                CompletedFetch::Package(package) => CompletedFetch::Package(package.clone()),
            })
    }

    pub(crate) fn cache_package(
        &self,
        package_id: &str,
        resolution: &LockfileResolution,
        package: &NativeFetcherResult,
    ) {
        let integrity = package.integrity
            .as_deref()
            .and_then(|value| value.parse().ok());
        let Some(identity) = fetch_identity(resolution, integrity.as_ref()) else { return };
        self.completed
            .lock()
            .unwrap()
            .insert(
                (package_id.to_owned(), identity.clone()),
                CompletedFetch::Package(package.clone()),
            );
        let Some(manifest) = &package.manifest else { return };
        let (Some(name), Some(version)) = (
            manifest.get("name").and_then(Value::as_str),
            manifest.get("version").and_then(Value::as_str),
        ) else {
            return;
        };
        let name_ver = format!("{name}@{version}");
        if name_ver != package_id {
            self.completed
                .lock()
                .unwrap()
                .insert((name_ver, identity), CompletedFetch::Package(package.clone()));
        }
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
