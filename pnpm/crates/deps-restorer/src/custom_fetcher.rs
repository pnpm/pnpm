mod callbacks;
use callbacks::run_callback;

use crate::{
    InstallPackageBySnapshotError, install_package_by_snapshot::local_file_tarball_install_url,
};
use pnpm_hooks::{CustomFetcher, custom_fetcher_adapter::CustomFetcherPicker};
use pnpm_lockfile::LockfileResolution;
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
    Delegate {
        resolution: LockfileResolution,
        delegate: LockfileResolution,
    },
    Fetched {
        resolution: LockfileResolution,
        tarball: Arc<FetchedTarball>,
    },
}

/// Shares verified custom fetches between fresh resolution and materialization.
pub struct CustomFetcherSession {
    picker: CustomFetcherPicker,
    completed: Mutex<HashMap<(String, String), Arc<FetchedTarball>>>,
}

impl CustomFetcherSession {
    #[must_use]
    pub fn new(fetchers: Vec<Arc<dyn CustomFetcher>>) -> Self {
        Self {
            picker: CustomFetcherPicker::new(fetchers),
            completed: Mutex::new(HashMap::new()),
        }
    }

    pub async fn resolve_tarball_integrity<Reporter: self::Reporter>(
        &self,
        download: IngestTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
    ) -> Result<LockfileResolution, InstallPackageBySnapshotError> {
        let lockfile_dir = fetcher_lockfile_dir(&opts, download.requester);
        let outcome = self.fetch::<Reporter>(download.clone(), original, opts).await?;
        let (resolution, tarball) =
            fetch_outcome_tarball::<Reporter>(&download, &lockfile_dir, outcome).await?;
        let Some(tarball) = tarball else {
            return Ok(resolution);
        };
        let resolution = decode_resolution(
            serde_json::json!(resolution),
            Some(&tarball.integrity),
            download.package.id,
        )?;
        self.completed
            .lock()
            .unwrap()
            .insert(
                (
                    download.package.id.to_owned(),
                    tarball.integrity.to_string(),
                ),
                tarball,
            );
        Ok(resolution)
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
            package: pnpm_tarball::TarballPackage {
                integrity: locked,
                ..download.package
            },
            ..download
        };
        if let Some(integrity) = locked
            && let Some(tarball) = self.completed
                .lock()
                .unwrap()
                .get(&(package_id.to_owned(), integrity.to_string()))
                .cloned()
        {
            return Ok(CustomFetchOutcome::Fetched {
                resolution: original.clone(),
                tarball,
            });
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
        let lockfile_dir = fetcher_lockfile_dir(&opts, download.requester);
        let (result, verified) = drive_fetcher::<Reporter>(
            fetcher,
            &download,
            &lockfile_dir,
            selected_resolution.clone(),
            opts,
        )
        .await?;
        decode_fetch_outcome(result, verified, selected_resolution, locked, package_id)
    }
}

/// Run the hook's fetch to completion, servicing the native-fetch
/// callbacks it makes along the way. The tarballs those callbacks
/// verified come back with the response so the caller can match the
/// files the hook claims against archives pacquet itself hashed.
async fn drive_fetcher<Reporter: self::Reporter>(
    fetcher: &dyn CustomFetcher,
    download: &IngestTarballToStore<'_>,
    lockfile_dir: &Path,
    selected_resolution: Value,
    opts: Value,
) -> Result<(Value, Vec<Arc<FetchedTarball>>), InstallPackageBySnapshotError> {
    let package_id = download.package.id;
    let (callbacks, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let fetch = fetcher.fetch_with_callbacks(package_id, selected_resolution, opts, callbacks);
    tokio::pin!(fetch);
    let mut verified = Vec::new();
    let result = loop {
        tokio::select! {
            result = &mut fetch => break result.map_err(|error| failure(package_id, error))?,
            Some(callback) = requests.recv() => {
                let result = run_callback::<Reporter>(download, lockfile_dir, &callback, &mut verified)
                    .await.map_err(|error| serde_json::json!(error));
                let _ = callback.response.send(result);
            }
        }
    };
    Ok((result, verified))
}

fn decode_fetch_outcome(
    result: Value,
    verified: Vec<Arc<FetchedTarball>>,
    selected_resolution: Value,
    locked: Option<&Integrity>,
    package_id: &str,
) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
    if result.get("filesMap").is_some() {
        let returned: ReturnedFiles =
            serde_json::from_value(result).map_err(|error| failure(package_id, error))?;
        let tarball = verified_tarball(&returned, verified, package_id)?;
        let resolution = decode_resolution(selected_resolution, None, package_id)?;
        return Ok(CustomFetchOutcome::Fetched {
            resolution,
            tarball,
        });
    }
    let Some(delegate) = result.get("delegate") else {
        return Err(failure(
            package_id,
            "unhandled response: expected a delegate or native fetched files",
        ));
    };
    validate_delegate(delegate, package_id)?;
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
        return Err(failure(
            package_id,
            "custom fetcher returned an ambiguous archive integrity",
        ));
    }
    Ok(tarball)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedFiles {
    files_map: HashMap<String, PathBuf>,
    integrity: Option<String>,
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
            return Err(
                InstallPackageBySnapshotError::CustomFetcherIntegrityMismatch {
                    package_id: package_id.to_owned(),
                },
            );
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| failure(package_id, "invalid resolution"))?;
        object.insert(
            "integrity".to_owned(),
            serde_json::json!(integrity.to_string()),
        );
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
/// digest — a directory or a git checkout. Only a fresh install's missing-digest
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
        LockfileResolution::Registry(resolution) => TarballLocation {
            tarball: download.package.url.to_owned(),
            integrity: Some(resolution.integrity.clone()),
        },
        _ => return Ok(None),
    };
    fetch_location::<Reporter>(&download, location, lockfile_dir).await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
        .map(Some)
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

fn validate_delegate(
    delegate: &Value,
    package_id: &str,
) -> Result<(), InstallPackageBySnapshotError> {
    if !["type", "tarball", "integrity"]
        .iter()
        .any(|key| delegate.get(key).is_some())
    {
        return Err(failure(package_id, "invalid delegate resolution"));
    }
    Ok(())
}

fn fetcher_lockfile_dir(opts: &Value, requester: &str) -> PathBuf {
    PathBuf::from(
        opts
            .get("lockfileDir")
            .and_then(Value::as_str)
            .unwrap_or(requester),
    )
}

async fn fetch_outcome_tarball<Reporter: self::Reporter>(
    download: &IngestTarballToStore<'_>,
    lockfile_dir: &Path,
    outcome: CustomFetchOutcome,
) -> Result<(LockfileResolution, Option<Arc<FetchedTarball>>), InstallPackageBySnapshotError> {
    let (resolution, delegate) = match outcome {
        CustomFetchOutcome::Fetched { resolution, tarball } => {
            return Ok((resolution, Some(tarball)));
        }
        CustomFetchOutcome::Declined(resolution) => (resolution, None),
        CustomFetchOutcome::Delegate { resolution, delegate } => (resolution, Some(delegate)),
    };
    let tarball = fetch_custom_tarball::<Reporter>(
        download.clone(),
        delegate.as_ref().unwrap_or(&resolution),
        lockfile_dir,
    )
    .await?;
    Ok((resolution, tarball))
}
