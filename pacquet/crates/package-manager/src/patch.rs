use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, InstallPackageBySnapshotError, import_indexed_dir,
    retry_config::retry_opts_from_config, tarball_url_and_integrity,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_config::{Config, PackageImportMethod, ScriptsPrependNodePath};
use pacquet_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pacquet_git_fetcher::{GitFetchOutput, GitFetcherError, GitHostedTarballFetcher};
use pacquet_lockfile::{Lockfile, LockfileResolution, PackageKey, is_git_hosted_tarball_url};
use pacquet_network::ThrottledClient;
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pacquet_store_dir::{SharedVerifiedFilesCache, git_hosted_store_index_key};
use pacquet_tarball::{DownloadTarballToStore, MemCache, TarballError};
use std::{cmp::Ordering, collections::BTreeSet, path::Path, sync::atomic::AtomicU8};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchCandidate {
    pub name: String,
    pub version: String,
    pub git_tarball_url: Option<String>,
    pub package_key: PackageKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchCandidateSet {
    pub alias: String,
    pub requested: String,
    pub bare_specifier: Option<String>,
    pub versions: Vec<PatchCandidate>,
    pub preferred_versions: Vec<PatchCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchTarget {
    pub alias: String,
    pub version: String,
    pub bare_specifier: String,
    pub apply_to_all: bool,
    pub git_tarball_url: Option<String>,
    pub package_key: PackageKey,
}

#[must_use]
pub struct WritePackageForPatch<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub current_lockfile: &'a Lockfile,
    pub target: &'a PatchTarget,
    pub dest: &'a Path,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PatchTargetError {
    #[display("Can not find {requested} in the current lockfile, {hint}")]
    #[diagnostic(code(ERR_PNPM_PATCH_VERSION_NOT_FOUND))]
    VersionNotFound { requested: String, hint: String },
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum WritePackageForPatchError {
    #[display("Package `{package_key}` is missing from the current lockfile packages map.")]
    #[diagnostic(code(pacquet_package_manager::patch_package_missing_metadata))]
    MissingPackageMetadata { package_key: String },

    #[display(
        "Package `{package}` uses a `{resolution_kind}` resolution, which pacquet patch does not yet support."
    )]
    #[diagnostic(code(pacquet_package_manager::patch_unsupported_resolution))]
    UnsupportedResolution { package: String, resolution_kind: &'static str },

    #[diagnostic(transparent)]
    TarballResolution(#[error(source)] InstallPackageBySnapshotError),

    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    GitFetch(#[error(source)] GitFetcherError),

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),
}

pub fn patch_candidates_from_lockfile(
    raw_dependency: &str,
    current_lockfile: &Lockfile,
) -> Result<PatchCandidateSet, PatchTargetError> {
    let parsed = parse_wanted_dependency(raw_dependency);
    let package_name =
        parsed.alias.as_deref().or(parsed.bare_specifier.as_deref()).unwrap_or(raw_dependency);
    let alias = parsed.alias.clone().unwrap_or_else(|| raw_dependency.to_string());
    let bare_specifier = parsed.bare_specifier.clone();

    let mut versions = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, metadata) in current_lockfile.packages.as_ref().into_iter().flatten() {
        let package_key = key.without_peer();
        let name = package_key.name.to_string();
        if name != package_name {
            continue;
        }
        let version =
            metadata.version.clone().unwrap_or_else(|| package_key.suffix.version().to_string());
        let git_tarball_url = git_tarball_url(&metadata.resolution);
        if seen.insert((name.clone(), version.clone(), git_tarball_url.clone())) {
            versions.push(PatchCandidate { name, version, git_tarball_url, package_key });
        }
    }
    versions.sort_by(compare_candidates);

    let preferred_versions = match bare_specifier.as_deref() {
        Some(specifier) => versions
            .iter()
            .filter(|candidate| version_satisfies(&candidate.version, specifier))
            .cloned()
            .collect(),
        None => versions.clone(),
    };

    if preferred_versions.is_empty() {
        return Err(PatchTargetError::VersionNotFound {
            requested: raw_dependency.to_string(),
            hint: version_not_found_hint(&versions, raw_dependency),
        });
    }

    Ok(PatchCandidateSet {
        alias,
        requested: raw_dependency.to_string(),
        bare_specifier,
        versions,
        preferred_versions,
    })
}

#[must_use]
pub fn default_patch_target(set: &PatchCandidateSet) -> Option<PatchTarget> {
    if set.preferred_versions.len() != 1 {
        return None;
    }
    let preferred = set.preferred_versions.first().expect("len checked");
    let bare_specifier =
        preferred.git_tarball_url.clone().unwrap_or_else(|| preferred.version.clone());
    Some(PatchTarget {
        alias: set.alias.clone(),
        version: preferred.version.clone(),
        bare_specifier,
        apply_to_all: set.bare_specifier.is_none() && preferred.git_tarball_url.is_none(),
        git_tarball_url: preferred.git_tarball_url.clone(),
        package_key: preferred.package_key.clone(),
    })
}

impl WritePackageForPatch<'_> {
    pub async fn run<Reporter: self::Reporter>(self) -> Result<(), WritePackageForPatchError> {
        let WritePackageForPatch {
            tarball_mem_cache,
            http_client,
            config,
            current_lockfile,
            target,
            dest,
        } = self;
        let metadata = current_lockfile
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&target.package_key))
            .ok_or_else(|| WritePackageForPatchError::MissingPackageMetadata {
                package_key: target.package_key.to_string(),
            })?;

        if !matches!(
            metadata.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            return Err(WritePackageForPatchError::UnsupportedResolution {
                package: format!("{}@{}", target.alias, target.version),
                resolution_kind: resolution_kind(&metadata.resolution),
            });
        }

        let (tarball_url, integrity) =
            tarball_url_and_integrity(&metadata.resolution, &target.package_key, config)
                .map_err(WritePackageForPatchError::TarballResolution)?;
        let package_id = format!("{}@{}", target.alias, target.version);
        let cas_paths = DownloadTarballToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: None,
            store_index_writer: None,
            verify_store_integrity: config.verify_store_integrity,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            package_integrity: integrity,
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &tarball_url,
            package_id: &package_id,
            auth_headers: &config.auth_headers,
            requester: "",
            prefetched_cas_paths: None,
            retry_opts: retry_opts_from_config(config),
            ignore_file_pattern: None,
            offline: config.offline,
            progress_reported: None,
        }
        .run_with_mem_cache::<Reporter>(tarball_mem_cache)
        .await
        .map_err(WritePackageForPatchError::DownloadTarball)?;
        let raw_cas_paths = (*cas_paths).clone();
        let cas_paths = if let LockfileResolution::Tarball(t) = &metadata.resolution
            && git_tarball_url(&metadata.resolution).is_some()
        {
            let allow_build_closure = |_dep_path: &str| false;
            let files_index_file = git_hosted_store_index_key(&package_id, !config.ignore_scripts);
            let GitFetchOutput { cas_paths, built: _built } = GitHostedTarballFetcher {
                cas_paths: raw_cas_paths,
                path: t.path.as_deref(),
                allow_build: &allow_build_closure,
                ignore_scripts: config.ignore_scripts,
                unsafe_perm: config.unsafe_perm,
                user_agent: None,
                scripts_prepend_node_path: executor_scripts_prepend_node_path(
                    config.scripts_prepend_node_path,
                ),
                script_shell: None,
                node_execpath: None,
                npm_execpath: None,
                store_dir: &config.store_dir,
                package_id: &package_id,
                requester: "",
                store_index_writer: None,
                files_index_file: &files_index_file,
            }
            .run::<Reporter>()
            .await
            .map_err(WritePackageForPatchError::GitFetch)?;
            cas_paths
        } else {
            raw_cas_paths
        };

        import_indexed_dir::<Reporter>(
            &AtomicU8::new(0),
            PackageImportMethod::CloneOrCopy,
            dest,
            &cas_paths,
            ImportIndexedDirOpts { force: true, keep_modules_dir: false },
        )
        .map_err(WritePackageForPatchError::ImportIndexedDir)
    }
}

fn git_tarball_url(resolution: &LockfileResolution) -> Option<String> {
    let LockfileResolution::Tarball(tarball) = resolution else { return None };
    (tarball.git_hosted == Some(true)
        || is_git_hosted_tarball_url(&tarball.tarball)
        || tarball.tarball.starts_with("https://pkg.pr.new/"))
    .then(|| tarball.tarball.clone())
}

fn executor_scripts_prepend_node_path(
    scripts_prepend_node_path: ScriptsPrependNodePath,
) -> ExecScriptsPrependNodePath {
    match scripts_prepend_node_path {
        ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
        ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
        ScriptsPrependNodePath::WarnOnly => ExecScriptsPrependNodePath::WarnOnly,
    }
}

fn version_satisfies(version: &str, range: &str) -> bool {
    let Ok(version) = Version::parse(version) else { return false };
    let Ok(range) = Range::parse(range) else { return false };
    version.satisfies(&range)
}

fn compare_candidates(left: &PatchCandidate, right: &PatchCandidate) -> Ordering {
    match (Version::parse(&left.version), Version::parse(&right.version)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => Ordering::Equal,
    }
}

fn version_not_found_hint(versions: &[PatchCandidate], raw_dependency: &str) -> String {
    if versions.is_empty() {
        format!("did you forget to install {raw_dependency}?")
    } else {
        format!(
            "you can specify currently installed version: {}.",
            versions
                .iter()
                .map(|candidate| candidate.version.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn resolution_kind(resolution: &LockfileResolution) -> &'static str {
    match resolution {
        LockfileResolution::Tarball(_) => "tarball",
        LockfileResolution::Registry(_) => "registry",
        LockfileResolution::Directory(_) => "directory",
        LockfileResolution::Git(_) => "git",
        LockfileResolution::Binary(_) => "binary",
        LockfileResolution::Variations(_) => "variations",
    }
}

#[cfg(test)]
mod tests;
