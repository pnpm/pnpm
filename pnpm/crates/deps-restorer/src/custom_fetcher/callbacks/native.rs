use super::{
    NativeCallbacks, NativeFetcherResult, NativeFetcherSettings, Verified, callback_error,
    callback_location, callback_lockfile_dir, fetch_location, reject_locked_integrity,
};
use crate::install_package_by_snapshot::{
    archive_filter_for, fetch_binary_resolution_to_cas, running_pnpm_execpath,
};
use pnpm_git_fetcher::{GitFetcher, GitHostedTarballFetcher, GitSource, GitStoreContext};
use pnpm_hooks::FetcherCallback;
use pnpm_lockfile::{GitResolution, PackageKey, TarballResolution};
use pnpm_store_dir::git_hosted_store_index_key;
use pnpm_tarball::{FetchErrorDetails, FetchedTarball};
use serde_json::Value;
use std::sync::Arc;

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
    let package_id = native.download.package.id;
    let (resolution, raw) = fetch_git_hosted_source::<Reporter>(native, callback).await?;
    let integrity = raw.integrity.to_string();
    let allow_build = |dep_path: &str| settings.allow_build_policy.check(dep_path);
    let files_index_file = git_hosted_files_index_file(settings, package_id, &allow_build);
    let output = GitHostedTarballFetcher {
        scripts: prepare_script_options(settings),
        store: GitStoreContext {
            dir: native.download.store.dir,
            index_writer: native.download.store.index_writer.as_ref(),
            files_index_file: &files_index_file,
        },
        cas_paths: Arc::unwrap_or_clone(raw).files_map,
        path: resolution.path.as_deref(),
        allow_build: &allow_build,
        package_id,
        requester: native.download.requester,
    }
    .run::<Reporter>()
    .await
    .map_err(|error| callback_error(error.to_string(), "ERR_PNPM_GIT_FETCH"))?;
    let result = NativeFetcherResult::from_files(output.cas_paths, Some(integrity));
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
    reject_locked_integrity(native.download, "git")?;
    let resolution: GitResolution = deserialize_resolution(&callback.resolution)?;
    let package_id = native.download.package.id;
    let package_name = package_name(callback)?;
    let allow_build = |dep_path: &str| settings.allow_build_policy.check(dep_path);
    let files_index_file = git_hosted_files_index_file(settings, package_id, &allow_build);
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
        package_id,
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
    if let Some(locked) = native.download.package.integrity
        && locked.matches(&resolution.integrity).is_none()
    {
        return Err(callback_error(
            "The binary archive integrity does not match the locked archive integrity.",
            "ERR_PNPM_TARBALL_INTEGRITY",
        ));
    }
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

/// The key the prepared files are indexed under, derived as the install pass
/// derives it. A key from the hook's options could name another package's
/// raw-archive entry.
fn git_hosted_files_index_file(
    settings: &NativeFetcherSettings,
    package_id: &str,
    allow_build: &impl Fn(&str) -> Option<bool>,
) -> String {
    let built = !settings.ignore_scripts && allow_build(package_id) != Some(false);
    git_hosted_store_index_key(package_id, built)
}
