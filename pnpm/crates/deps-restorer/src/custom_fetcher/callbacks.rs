//! The native callbacks a hook's `fetch` makes through its `cafs` and
//! `fetchers` arguments.

mod native;

use super::{TarballLocation, failure, fetch_location};
use crate::{
    AllowBuildPolicy, InstallPackageBySnapshotError, install_package_by_snapshot::walk_directory,
};
use miette::Diagnostic;
use pnpm_config::{Config, PmOnFail};
use pnpm_directory_fetcher::CopyFiles;
use pnpm_fs::lexical_normalize;
use pnpm_hooks::{CustomFetcher, FetcherCallback, FetcherMethod};
use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_reporter::Reporter;
use pnpm_tarball::{FetchErrorDetails, FetchedTarball, IngestTarballToStore};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct NativeFetcherSettings {
    ignore_scripts: bool,
    unsafe_perm: bool,
    user_agent: String,
    pm_on_fail: Option<String>,
    git_shallow_hosts: Vec<String>,
    allow_build_policy: AllowBuildPolicy,
}

impl NativeFetcherSettings {
    pub(super) fn from_config(config: &Config) -> Result<Self, crate::VersionPolicyError> {
        Ok(Self {
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: config.user_agent.clone(),
            pm_on_fail: config.pm_on_fail.map(PmOnFail::as_str).map(ToString::to_string),
            git_shallow_hosts: config.git_shallow_hosts.clone(),
            allow_build_policy: AllowBuildPolicy::from_config(config)?,
        })
    }
}

#[derive(Clone)]
pub(crate) struct NativeFetcherResult {
    pub(crate) files_map: HashMap<String, PathBuf>,
    pub(crate) integrity: Option<String>,
    pub(crate) local: bool,
    pub(crate) manifest: Option<Value>,
    pub(crate) requires_build: bool,
    pub(crate) bins: Option<std::collections::BTreeMap<String, pnpm_package_manifest::PackageBin>>,
}

impl NativeFetcherResult {
    fn value(&self) -> Value {
        serde_json::json!({
            "filesMap": &self.files_map,
            "integrity": &self.integrity,
            "local": self.local,
            "manifest": &self.manifest,
            "requiresBuild": self.requires_build,
        })
    }

    fn from_files(files_map: HashMap<String, PathBuf>, integrity: Option<String>) -> Self {
        let manifest = pnpm_package_manifest::read_package_json_from_cas_paths(&files_map);
        let requires_build = pnpm_package_manifest::requires_build_from_cas_paths(&files_map);
        let bins = manifest
            .as_ref()
            .and_then(|manifest| {
                pnpm_package_manifest::resolve_package_bins_from_cas_paths(manifest, &files_map)
            });
        Self { files_map, integrity, local: false, manifest, requires_build, bins }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReturnedNativeFiles {
    pub(super) files_map: HashMap<String, PathBuf>,
    pub(super) integrity: Option<String>,
}

/// What the native callbacks of one fetch run against.
pub(super) struct NativeCallbacks<'a> {
    pub(super) download: &'a IngestTarballToStore<'a>,
    pub(super) lockfile_dir: &'a Path,
    pub(super) files: CopyFiles,
    pub(super) settings: Option<&'a NativeFetcherSettings>,
    pub(super) git_source_cache: &'a pnpm_git_fetcher::GitSourceCache,
}

/// What the native callbacks of one fetch produced. A hook may only return
/// files listed here.
#[derive(Default)]
pub(super) struct Verified {
    pub(super) archives: Vec<Arc<FetchedTarball>>,
    pub(super) directories: Vec<WalkedDirectory>,
    pub(super) packages: Vec<NativeFetcherResult>,
}

/// A directory the `fetchers.directory` callback walked, with the install
/// pass's options.
pub(crate) struct WalkedDirectory {
    pub(crate) directory: String,
    pub(crate) files_map: HashMap<String, PathBuf>,
    pub(crate) manifest: Option<Value>,
    /// See [`crate::SlotImportSource::source_exists`].
    pub(crate) exists: bool,
}

impl WalkedDirectory {
    pub(crate) fn resolution(&self) -> LockfileResolution {
        LockfileResolution::Directory(DirectoryResolution { directory: self.directory.clone() })
    }
}

/// Run the hook's fetch to completion, servicing the native-fetch
/// callbacks it makes along the way. What those callbacks produced comes
/// back with the response so the caller can match the files the hook
/// claims against archives pacquet itself hashed or directories it walked.
pub(super) async fn drive_fetcher<Reporter: self::Reporter>(
    fetcher: &dyn CustomFetcher,
    native: &NativeCallbacks<'_>,
    selected_resolution: Value,
    opts: Value,
) -> Result<(Value, Verified), InstallPackageBySnapshotError> {
    let package_id = native.download.package.id;
    let (callbacks, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let fetch = fetcher.fetch_with_callbacks(package_id, selected_resolution, opts, callbacks);
    tokio::pin!(fetch);
    let mut verified = Verified::default();
    let result = loop {
        tokio::select! {
            result = &mut fetch => break result.map_err(|error| failure(package_id, error))?,
            Some(callback) = requests.recv() => {
                let result = run_callback::<Reporter>(native, &callback, &mut verified)
                    .await.map_err(|error| serde_json::json!(error));
                let _ = callback.response.send(result);
            }
        }
    };
    Ok((result, verified))
}

fn callback_error(message: impl Into<String>, code: &str) -> FetchErrorDetails {
    FetchErrorDetails { message: message.into(), code: Some(code.to_owned()), status: None }
}

async fn run_callback<Reporter: self::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    verified: &mut Verified,
) -> Result<Value, FetchErrorDetails> {
    let download = native.download;
    let expects_local_archive = match callback.method {
        FetcherMethod::CafsInfo => {
            return Ok(serde_json::json!({ "storeDir": download.store.dir.root() }));
        }
        FetcherMethod::TempDir => return temp_dir(download).await,
        FetcherMethod::Directory => {
            return walk_callback_directory(native, callback, &mut verified.directories);
        }
        FetcherMethod::GitHostedTarball | FetcherMethod::Git | FetcherMethod::Binary => {
            return native::run_native_callback::<Reporter>(native, callback, verified).await;
        }
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
    let tarball =
        fetch_location::<Reporter>(download, location, callback_lockfile_dir(native, callback))
            .await
            .map_err(|error| error.fetch_error_details())?;
    let result = serde_json::json!({
        "filesMap": tarball.files_map,
        "integrity": tarball.integrity.to_string(),
        "manifest": tarball.manifest,
        "requiresBuild": tarball.requires_build,
    });
    verified.archives.push(tarball);
    Ok(result)
}

fn callback_lockfile_dir<'a>(
    native: &NativeCallbacks<'a>,
    callback: &'a FetcherCallback,
) -> &'a Path {
    callback.options
        .get("lockfileDir")
        .and_then(Value::as_str)
        .map_or(native.lockfile_dir, Path::new)
}

/// `fetchers.directory`. A directory has no archive digest, so when the
/// lockfile pins one the call itself rejects, which lets the hook catch it.
fn walk_callback_directory(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    walked: &mut Vec<WalkedDirectory>,
) -> Result<Value, FetchErrorDetails> {
    if native.download.package.integrity.is_some() {
        return Err(callback_error(
            r#"Cannot verify the locked archive integrity after delegating to resolution type "directory"."#,
            "ERR_PNPM_TARBALL_INTEGRITY",
        ));
    }
    // Like the TypeScript directory fetcher, read `directory` alone: a hook
    // often spreads its own resolution, which carries other fields.
    let Some(relative) = callback.resolution.get("directory").and_then(Value::as_str) else {
        return Err(callback_error(
            "native directory callback received a resolution without a directory",
            "ERR_PNPM_INVALID_FETCHER_RESOLUTION",
        ));
    };
    let directory = lexical_normalize(&callback_lockfile_dir(native, callback).join(relative));
    let output = walk_directory(&directory, native.files)
        .map_err(|error| FetchErrorDetails {
            message: error.to_string(),
            code: error.code().map(|code| code.to_string()),
            status: None,
        })?;
    let result = serde_json::json!({
        "local": true,
        "filesMap": output.files_map,
        "manifest": output.manifest,
        "requiresBuild": output.requires_build,
        "sourceExists": output.exists,
    });
    walked.push(WalkedDirectory {
        directory: directory.to_string_lossy().into_owned(),
        files_map: output.files_map,
        manifest: output.manifest,
        exists: output.exists,
    });
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
        object.insert("integrity".to_owned(), serde_json::json!(integrity.to_string()));
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
