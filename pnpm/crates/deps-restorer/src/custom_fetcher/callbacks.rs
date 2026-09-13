use super::{TarballLocation, fetch_location};
use pnpm_hooks::{FetcherCallback, FetcherMethod};
use pnpm_reporter::Reporter;
use pnpm_tarball::{FetchErrorDetails, FetchedTarball, IngestTarballToStore};
use serde_json::Value;
use std::{path::Path, sync::Arc};

fn callback_error(message: impl Into<String>, code: &str) -> FetchErrorDetails {
    FetchErrorDetails {
        message: message.into(),
        code: Some(code.to_owned()),
        status: None,
    }
}

pub(super) async fn run_callback<Reporter: self::Reporter>(
    download: &IngestTarballToStore<'_>,
    lockfile_dir: &Path,
    callback: &FetcherCallback,
    verified: &mut Vec<Arc<FetchedTarball>>,
) -> Result<Value, FetchErrorDetails> {
    let expects_local_archive = match callback.method {
        FetcherMethod::CafsInfo => {
            return Ok(serde_json::json!({ "storeDir": download.store.dir.root() }));
        }
        FetcherMethod::TempDir => return temp_dir(download).await,
        FetcherMethod::LocalTarball => true,
        FetcherMethod::RemoteTarball => false,
    };
    for option in ["ignoreFilePattern", "appendManifest"] {
        if callback.options
            .get(option)
            .is_some_and(|value| !value.is_null())
        {
            return Err(callback_error(
                format!("native custom-fetcher callbacks do not support {option}"),
                "ERR_PNPM_UNSUPPORTED_FETCHER_OPTION",
            ));
        }
    }
    let location = callback_location(callback, download, expects_local_archive)?;
    let lockfile_dir = callback.options
        .get("lockfileDir")
        .and_then(Value::as_str)
        .map_or(lockfile_dir, Path::new);
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

/// A fresh directory under the store's temp root, kept for the fetcher.
async fn temp_dir(download: &IngestTarballToStore<'_>) -> Result<Value, FetchErrorDetails> {
    let root = download.store.dir.tmp();
    tokio::fs::create_dir_all(&root).await
        .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_FETCHER_TEMP_DIR"))?;
    let directory = tempfile::Builder::new()
        .prefix("fetcher-")
        .tempdir_in(root)
        .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_FETCHER_TEMP_DIR"))?
        .keep();
    Ok(serde_json::json!(directory))
}

/// The callback's resolution as a tarball location, carrying the
/// download's integrity when the callback named none.
///
/// Each callback answers for one transport, so the URL has to name that
/// transport and no other. Without the positive test on the remote
/// side, a scheme neither fetcher handles — `ftp:`, `data:`, a bare
/// path — counts as remote and fails deep in the HTTP client instead of
/// here.
fn callback_location(
    callback: &FetcherCallback,
    download: &IngestTarballToStore<'_>,
    expects_local_archive: bool,
) -> Result<TarballLocation, FetchErrorDetails> {
    let mut location = callback.resolution.clone();
    if let Some(integrity) = download.package.integrity
        && let Some(object) = location.as_object_mut()
    {
        object.insert(
            "integrity".to_owned(),
            serde_json::json!(integrity.to_string()),
        );
    }
    let location: TarballLocation = serde_json::from_value(location)
        .map_err(|error| {
            callback_error(error.to_string(), "ERR_PNPM_INVALID_FETCHER_RESOLUTION")
        })?;
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
    Ok(location)
}
