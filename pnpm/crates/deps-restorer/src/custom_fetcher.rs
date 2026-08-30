use crate::{
    InstallPackageBySnapshotError, install_package_by_snapshot::local_file_tarball_install_url,
};
use pnpm_hooks::{
    CustomFetcher, FetcherCallback, FetcherMethod, custom_fetcher_adapter::CustomFetcherPicker,
};
use pnpm_lockfile::LockfileResolution;
use pnpm_reporter::Reporter;
use pnpm_tarball::{DownloadTarballToStore, FetchErrorDetails, FetchedTarball, TarballError};
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

/// Shares verified custom fetches between fresh resolution and materialization.
pub struct CustomFetcherSession {
    picker: CustomFetcherPicker,
    completed: Mutex<HashMap<(String, String), Arc<FetchedTarball>>>,
}

impl CustomFetcherSession {
    #[must_use]
    pub fn new(fetchers: Vec<Arc<dyn CustomFetcher>>) -> Self {
        Self { picker: CustomFetcherPicker::new(fetchers), completed: Mutex::new(HashMap::new()) }
    }

    pub async fn resolve_tarball_integrity<Reporter: self::Reporter>(
        &self,
        download: DownloadTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
    ) -> Result<LockfileResolution, InstallPackageBySnapshotError> {
        let lockfile_dir = PathBuf::from(
            opts.get("lockfileDir").and_then(Value::as_str).unwrap_or(download.requester),
        );
        let (resolution, tarball) = match self
            .fetch::<Reporter>(download.clone(), original, opts)
            .await?
        {
            CustomFetchOutcome::Fetched { resolution, tarball } => (resolution, tarball),
            CustomFetchOutcome::Declined(resolution) => {
                let Some(tarball) =
                    fetch_custom_tarball::<Reporter>(download.clone(), &resolution, &lockfile_dir)
                        .await?
                else {
                    return Ok(resolution);
                };
                (resolution, tarball)
            }
            CustomFetchOutcome::Delegate { resolution, delegate } => {
                let Some(tarball) =
                    fetch_custom_tarball::<Reporter>(download.clone(), &delegate, &lockfile_dir)
                        .await?
                else {
                    return Ok(resolution);
                };
                (resolution, tarball)
            }
        };
        let resolution = decode_resolution(
            serde_json::json!(resolution),
            Some(&tarball.integrity),
            download.package_id,
        )?;
        self.completed
            .lock()
            .unwrap()
            .insert((download.package_id.to_owned(), tarball.integrity.to_string()), tarball);
        Ok(resolution)
    }

    pub(crate) async fn fetch<Reporter: self::Reporter>(
        &self,
        download: DownloadTarballToStore<'_>,
        original: &LockfileResolution,
        opts: Value,
    ) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
        let package_id = download.package_id;
        let locked = original.checkable_integrity();
        let download = DownloadTarballToStore { package_integrity: locked, ..download };
        if let Some(integrity) = locked
            && let Some(tarball) = self
                .completed
                .lock()
                .unwrap()
                .get(&(package_id.to_owned(), integrity.to_string()))
                .cloned()
        {
            return Ok(CustomFetchOutcome::Fetched { resolution: original.clone(), tarball });
        }
        let selection = self
            .picker
            .pick_fetcher(package_id, &serde_json::json!(original))
            .await
            .map_err(|error| failure(package_id, error))?;
        let Some(fetcher) = selection.fetcher else {
            return decode_resolution(selection.resolution, locked, package_id)
                .map(CustomFetchOutcome::Declined);
        };
        let selected_resolution = selection.resolution;
        let lockfile_dir = PathBuf::from(
            opts.get("lockfileDir").and_then(Value::as_str).unwrap_or(download.requester),
        );
        let (callbacks, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let fetch =
            fetcher.fetch_with_callbacks(package_id, selected_resolution.clone(), opts, callbacks);
        tokio::pin!(fetch);
        let mut verified = Vec::new();
        let result = loop {
            tokio::select! {
                result = &mut fetch => break result.map_err(|error| failure(package_id, error))?,
                Some(callback) = requests.recv() => {
                    let result = run_callback::<Reporter>(&download, &lockfile_dir, &callback, &mut verified)
                        .await.map_err(|error| serde_json::json!(error));
                    let _ = callback.response.send(result);
                }
            }
        };
        if result.get("filesMap").is_some() {
            let returned: ReturnedFiles =
                serde_json::from_value(result).map_err(|error| failure(package_id, error))?;
            let mut matches = verified.into_iter().filter(|verified: &Arc<FetchedTarball>| {
                verified.files_map == returned.files_map
                    && returned
                        .integrity
                        .as_ref()
                        .is_none_or(|integrity| integrity == &verified.integrity.to_string())
            });
            let tarball = matches.next().ok_or_else(|| {
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
            let resolution = decode_resolution(selected_resolution, None, package_id)?;
            Ok(CustomFetchOutcome::Fetched { resolution, tarball })
        } else if let Some(delegate) = result.get("delegate") {
            if !["type", "tarball", "integrity"].iter().any(|key| delegate.get(key).is_some()) {
                return Err(failure(package_id, "invalid delegate resolution"));
            }
            Ok(CustomFetchOutcome::Delegate {
                resolution: decode_resolution(selected_resolution, None, package_id)?,
                delegate: decode_resolution(delegate.clone(), locked, package_id).map_err(
                    |error| match error {
                        InstallPackageBySnapshotError::CustomFetcher(message) => {
                            InstallPackageBySnapshotError::CustomFetcher(format!(
                                "invalid delegate resolution: {message}",
                            ))
                        }
                        error => error,
                    },
                )?,
            })
        } else {
            Err(failure(
                package_id,
                "unhandled response: expected a delegate or native fetched files",
            ))
        }
    }
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

fn callback_error(message: impl Into<String>, code: &str) -> FetchErrorDetails {
    FetchErrorDetails { message: message.into(), code: Some(code.to_owned()), status: None }
}

fn decode_resolution(
    mut value: Value,
    locked: Option<&Integrity>,
    package_id: &str,
) -> Result<LockfileResolution, InstallPackageBySnapshotError> {
    if let Some(integrity) = locked {
        if !value.get("type").is_none_or(|kind| kind.is_null() || kind == "binary") {
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
/// digest — a directory or a git checkout. Only a fresh install's missing-digest
/// discovery calls this, so there is nothing to hash and nothing to verify; the
/// install pass materializes such a resolution through its own dispatch.
async fn fetch_custom_tarball<Reporter: self::Reporter>(
    download: DownloadTarballToStore<'_>,
    resolution: &LockfileResolution,
    lockfile_dir: &Path,
) -> Result<Option<Arc<FetchedTarball>>, InstallPackageBySnapshotError> {
    let location = match resolution {
        LockfileResolution::Tarball(resolution) => TarballLocation {
            tarball: resolution.tarball.clone(),
            integrity: resolution.integrity.clone(),
        },
        LockfileResolution::Registry(resolution) => TarballLocation {
            tarball: download.package_url.to_owned(),
            integrity: Some(resolution.integrity.clone()),
        },
        _ => return Ok(None),
    };
    fetch_location::<Reporter>(&download, location, lockfile_dir)
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)
        .map(Some)
}

async fn fetch_location<Reporter: self::Reporter>(
    download: &DownloadTarballToStore<'_>,
    location: TarballLocation,
    lockfile_dir: &Path,
) -> Result<Arc<FetchedTarball>, TarballError> {
    let url = local_file_tarball_install_url(location.tarball.as_str().into(), lockfile_dir);
    DownloadTarballToStore {
        package_url: &url,
        package_integrity: download
            .package_integrity
            .or_else(|| location.integrity.as_ref().filter(|value| !value.hashes.is_empty())),
        ..download.clone()
    }
    .fetch_and_extract::<Reporter>()
    .await
    .map(Arc::new)
}

async fn run_callback<Reporter: self::Reporter>(
    download: &DownloadTarballToStore<'_>,
    lockfile_dir: &Path,
    callback: &FetcherCallback,
    verified: &mut Vec<Arc<FetchedTarball>>,
) -> Result<Value, FetchErrorDetails> {
    let expects_local_archive = match callback.method {
        FetcherMethod::CafsInfo => {
            return Ok(serde_json::json!({ "storeDir": download.store_dir.root() }));
        }
        FetcherMethod::TempDir => {
            let root = download.store_dir.tmp();
            tokio::fs::create_dir_all(&root)
                .await
                .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_FETCHER_TEMP_DIR"))?;
            let directory = tempfile::Builder::new()
                .prefix("fetcher-")
                .tempdir_in(root)
                .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_FETCHER_TEMP_DIR"))?
                .keep();
            return Ok(serde_json::json!(directory));
        }
        FetcherMethod::LocalTarball => true,
        FetcherMethod::RemoteTarball => false,
    };
    for option in ["ignoreFilePattern", "appendManifest"] {
        if callback.options.get(option).is_some_and(|value| !value.is_null()) {
            return Err(callback_error(
                format!("native custom-fetcher callbacks do not support {option}"),
                "ERR_PNPM_UNSUPPORTED_FETCHER_OPTION",
            ));
        }
    }
    let mut location = callback.resolution.clone();
    if let Some(integrity) = download.package_integrity
        && let Some(object) = location.as_object_mut()
    {
        object.insert("integrity".to_owned(), serde_json::json!(integrity.to_string()));
    }
    let location: TarballLocation = serde_json::from_value(location).map_err(|error| {
        callback_error(error.to_string(), "ERR_PNPM_INVALID_FETCHER_RESOLUTION")
    })?;
    // Each callback answers for one transport, so the URL has to name that
    // transport and no other. Without the positive test on the remote side, a
    // scheme neither fetcher handles — `ftp:`, `data:`, a bare path — counts as
    // remote and fails deep in the HTTP client instead of here.
    let scheme_matches_callback = if expects_local_archive {
        location.tarball.starts_with("file:")
    } else {
        location.tarball.starts_with("https:") || location.tarball.starts_with("http:")
    };
    if !scheme_matches_callback {
        return Err(callback_error(
            "native tarball callback received an incompatible URL",
            "ERR_PNPM_INVALID_FETCHER_RESOLUTION",
        ));
    }
    let lockfile_dir =
        callback.options.get("lockfileDir").and_then(Value::as_str).map_or(lockfile_dir, Path::new);
    let tarball = fetch_location::<Reporter>(download, location, lockfile_dir)
        .await
        .map_err(|error| error.fetch_error_details())?;
    let result = serde_json::json!({
        "filesMap": tarball.files_map,
        "integrity": tarball.integrity.to_string(),
        "manifest": tarball.manifest,
        "requiresBuild": tarball.requires_build,
    });
    verified.push(tarball);
    Ok(result)
}
