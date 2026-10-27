use super::{
    CustomFetchOutcome, NativeFetcherResult, Verified, callbacks, callbacks::WalkedDirectory,
    failure,
};
use crate::InstallPackageBySnapshotError;
use pnpm_lockfile::LockfileResolution;
use pnpm_tarball::FetchedTarball;
use serde_json::Value;
use ssri::Integrity;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub(super) fn decode_fetch_outcome(
    result: Value,
    verified: Verified,
    selected_resolution: Value,
    locked: Option<&Integrity>,
    package_id: &str,
) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
    if result.get("filesMap").is_some() {
        return decode_fetched_files(result, verified, selected_resolution, package_id);
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

fn decode_fetched_files(
    result: Value,
    verified: Verified,
    selected_resolution: Value,
    package_id: &str,
) -> Result<CustomFetchOutcome, InstallPackageBySnapshotError> {
    if let Ok(returned) = serde_json::from_value::<callbacks::ReturnedNativeFiles>(result.clone())
        && let Some(package) = verified_package(&returned, verified.packages, package_id)?
    {
        return Ok(CustomFetchOutcome::FetchedFiles {
            resolution: decode_resolution(selected_resolution, None, package_id)?,
            package,
        });
    }
    let returned: ReturnedFiles =
        serde_json::from_value(result).map_err(|error| failure(package_id, error))?;
    let resolution = decode_resolution(selected_resolution, None, package_id)?;
    if returned.local {
        return Ok(CustomFetchOutcome::Walked {
            directory: walked_directory(&returned, verified.directories, package_id)?,
            resolution,
        });
    }
    let tarball = verified_tarball(&returned, verified.archives, package_id)?;
    Ok(CustomFetchOutcome::Fetched { resolution, tarball })
}

fn verified_package(
    returned: &callbacks::ReturnedNativeFiles,
    verified: Vec<NativeFetcherResult>,
    package_id: &str,
) -> Result<Option<NativeFetcherResult>, InstallPackageBySnapshotError> {
    let mut matches = verified
        .into_iter()
        .filter(|verified| {
            verified.files_map == returned.files_map
                && returned.integrity
                    .as_ref()
                    .is_none_or(|integrity| verified.integrity.as_ref() == Some(integrity))
        });
    let Some(package) = matches.next() else {
        return Ok(None);
    };
    if matches.any(|other| other.integrity != package.integrity) {
        return Err(failure(package_id, "custom fetcher returned ambiguous native files"));
    }
    Ok(Some(package))
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
) -> Result<WalkedDirectory, InstallPackageBySnapshotError> {
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
        })?;
    if matches.any(|other| other.directory != directory.directory) {
        return Err(failure(package_id, "custom fetcher returned an ambiguous directory"));
    }
    Ok(directory)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedFiles {
    files_map: HashMap<String, PathBuf>,
    integrity: Option<String>,
    #[serde(default)]
    local: bool,
}

pub(super) fn decode_resolution(
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
