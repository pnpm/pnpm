use super::{
    NativeCallbacks, NativeFetcherResult, NativeFetcherSettings, Verified, callback_error,
    callback_location, callback_lockfile_dir, fetch_location,
};
use crate::install_package_by_snapshot::{
    archive_filter_for, fetch_binary_resolution_to_cas, running_pnpm_execpath,
};
use pnpm_git_fetcher::{GitFetcher, GitHostedTarballFetcher, GitSource, GitStoreContext};
use pnpm_hooks::{FetcherCallback, FetcherMethod};
use pnpm_lockfile::{GitResolution, PackageKey, TarballResolution};
use pnpm_store_dir::git_hosted_store_index_key;
use pnpm_tarball::{FetchErrorDetails, FetchedTarball, IngestTarballToStore};
use serde_json::Value;
use std::sync::Arc;

pub(super) async fn run_native_callback<Reporter: pnpm_reporter::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    verified: &mut Verified,
) -> Result<Value, FetchErrorDetails> {
    match callback.method {
        FetcherMethod::GitHostedTarball => {
            fetch_git_hosted_tarball::<Reporter>(native, callback, verified).await
        }
        FetcherMethod::Git => fetch_git::<Reporter>(native, callback, verified).await,
        FetcherMethod::Binary => fetch_binary::<Reporter>(native, callback, verified).await,
        _ => unreachable!(),
    }
}

pub(super) async fn fetch_git_hosted_tarball<Reporter: pnpm_reporter::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    verified: &mut Verified,
) -> Result<Value, FetchErrorDetails> {
    let settings = native.settings.ok_or_else(|| {
        callback_error(
            "git-hosted tarball fetcher requires install settings",
            "ERR_PNPM_CUSTOM_FETCHER",
        )
    })?;
    let package_id = package_resolution_id(callback, native.download);
    let package_name = package_name(callback)?;
    let (resolution, raw) = fetch_git_hosted_source::<Reporter>(native, callback).await?;
    let allow_build = |dep_path: &str| settings.allow_build_policy.check(dep_path);
    let files_index_file =
        git_hosted_files_index_file(callback, settings, &package_name, &package_id, &allow_build);
    let output = GitHostedTarballFetcher {
        scripts: prepare_script_options(settings),
        store: GitStoreContext {
            dir: native.download.store.dir,
            index_writer: native.download.store.index_writer.as_ref(),
            files_index_file: &files_index_file,
        },
        cas_paths: raw.files_map.clone(),
        path: resolution.path.as_deref(),
        allow_build: &allow_build,
        package_id: &package_id,
        requester: native.download.requester,
    }
    .run::<Reporter>()
    .await
    .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_GIT_FETCH"))?;
    let result = NativeFetcherResult::from_files(output.cas_paths, Some(raw.integrity.to_string()));
    let value = result.value();
    verified.packages.push(result);
    Ok(value)
}

pub(super) async fn fetch_git<Reporter: pnpm_reporter::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    verified: &mut Verified,
) -> Result<Value, FetchErrorDetails> {
    let settings = native.settings.ok_or_else(|| {
        callback_error("git fetcher requires install settings", "ERR_PNPM_CUSTOM_FETCHER")
    })?;
    let resolution: GitResolution = deserialize_resolution(&callback.resolution)?;
    let package_id = package_resolution_id(callback, native.download);
    let package_name = package_name(callback)?;
    let allow_build = |dep_path: &str| settings.allow_build_policy.check(dep_path);
    let files_index_file =
        git_hosted_files_index_file(callback, settings, &package_name, &package_id, &allow_build);
    let output = GitFetcher {
        scripts: prepare_script_options(settings),
        source: GitSource {
            cache: native.git_source_cache,
            repo: &resolution.repo,
            commit: &resolution.commit,
            path: resolution.path.as_deref(),
            shallow_hosts: &settings.git_shallow_hosts,
            git_bin: None,
        },
        store: GitStoreContext {
            dir: native.download.store.dir,
            index_writer: native.download.store.index_writer.as_ref(),
            files_index_file: &files_index_file,
        },
        allow_build: &allow_build,
        package_id: &package_id,
        package_name: &package_name,
        requester: native.download.requester,
    }
    .run::<Reporter>()
    .await
    .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_GIT_FETCH"))?;
    let result = NativeFetcherResult::from_files(output.cas_paths, None);
    let value = result.value();
    verified.packages.push(result);
    Ok(value)
}

pub(super) async fn fetch_binary<Reporter: pnpm_reporter::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
    verified: &mut Verified,
) -> Result<Value, FetchErrorDetails> {
    let resolution: pnpm_lockfile::BinaryResolution = deserialize_resolution(&callback.resolution)?;
    let name = package_name(callback)?;
    let version = callback.options
        .get("pkg")
        .and_then(|pkg| pkg.get("version"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            callback_error(
                "binary fetcher requires a package version",
                "ERR_PNPM_INVALID_FETCHER_OPTIONS",
            )
        })?;
    let package_key = format!("{name}@{version}")
        .parse::<PackageKey>()
        .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_INVALID_FETCHER_OPTIONS"))?;
    let files_map = fetch_binary_resolution_to_cas::<Reporter>(
        &resolution,
        native.download,
        &package_key,
        archive_filter_for(&package_key),
    )
    .await
    .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_BINARY_FETCH"))?;
    let result = NativeFetcherResult::from_files(files_map, Some(resolution.integrity.to_string()));
    let value = result.value();
    verified.packages.push(result);
    Ok(value)
}

fn prepare_script_options(
    settings: &NativeFetcherSettings,
) -> pnpm_git_fetcher::PrepareScriptOptions<'_> {
    pnpm_git_fetcher::PrepareScriptOptions {
        ignore: settings.ignore_scripts,
        unsafe_perm: settings.unsafe_perm,
        user_agent: Some(&settings.user_agent),
        shell: None,
        node_execpath: None,
        npm_execpath: None,
        running_pnpm: pnpm_git_fetcher::RunningPnpm {
            execpath: running_pnpm_execpath(),
            pm_on_fail: settings.pm_on_fail.as_deref(),
        },
    }
}

fn package_resolution_id(
    callback: &FetcherCallback,
    download: &IngestTarballToStore<'_>,
) -> String {
    callback.options
        .get("pkgResolutionId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| download.package.id.to_string())
}

fn package_name(callback: &FetcherCallback) -> Result<String, FetchErrorDetails> {
    callback.options
        .get("pkg")
        .and_then(|pkg| pkg.get("name"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            callback_error(
                "built-in fetcher requires a package name",
                "ERR_PNPM_INVALID_FETCHER_OPTIONS",
            )
        })
}

fn deserialize_resolution<Resolution: serde::de::DeserializeOwned>(
    resolution: &Value,
) -> Result<Resolution, FetchErrorDetails> {
    let mut resolution = resolution.clone();
    if let Some(object) = resolution.as_object_mut() {
        object.remove("type");
    }
    serde_json::from_value(resolution)
        .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_INVALID_FETCHER_RESOLUTION"))
}

async fn fetch_git_hosted_source<Reporter: pnpm_reporter::Reporter>(
    native: &NativeCallbacks<'_>,
    callback: &FetcherCallback,
) -> Result<(TarballResolution, Arc<FetchedTarball>), FetchErrorDetails> {
    let resolution = deserialize_resolution(&callback.resolution)?;
    let location = callback_location(callback, native.download, false)?;
    let raw = fetch_location::<Reporter>(
        native.download,
        location,
        callback_lockfile_dir(native, callback),
    )
    .await
    .map_err(|error| error.fetch_error_details())?;
    Ok((resolution, raw))
}

fn git_hosted_files_index_file(
    callback: &FetcherCallback,
    settings: &NativeFetcherSettings,
    package_name: &str,
    package_id: &str,
    allow_build: &impl Fn(&str) -> Option<bool>,
) -> String {
    callback.options
        .get("filesIndexFile")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            let package_key = format!("{package_name}@{package_id}");
            let built = !settings.ignore_scripts && allow_build(&package_key) != Some(false);
            git_hosted_store_index_key(package_id, built)
        })
}
