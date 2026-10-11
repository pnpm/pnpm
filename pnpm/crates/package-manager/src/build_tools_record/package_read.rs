//! Read a git-hosted package at its locked commit to learn what its build
//! needs.

use super::RecordBuildToolsError;
use pnpm_config::Config;
use pnpm_git_fetcher::{
    BuildEnvironment, CasPackageFiles, GitManifestQuery, GitSourceCache, detect_build_environment,
    read_git_build_environment,
};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, TarballResolution};
use pnpm_network::ThrottledClient;
use pnpm_reporter::Reporter;
use pnpm_store_dir::SharedVerifiedFilesCache;
use pnpm_tarball::{IngestTarballToStore, MemCache};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Where a git-hosted package's files come from: the checkout the install
/// shares between resolution and fetch, or the archive a git host serves,
/// read through the install's tarball cache so it is downloaded once.
#[derive(Clone, Copy)]
pub(crate) struct PackageSources<'a> {
    pub config: &'static Config,
    pub http_client: &'a ThrottledClient,
    pub git_source_cache: &'a GitSourceCache,
    pub tarball_mem_cache: &'a MemCache,
}

impl PackageSources<'_> {
    /// What the package at `metadata_key` needs to be built. `None` when
    /// it needs no build.
    pub(super) async fn read_build_environment<Reporter: self::Reporter>(
        &self,
        metadata_key: &PackageKey,
        metadata: &PackageMetadata,
    ) -> Result<Option<BuildEnvironment>, RecordBuildToolsError> {
        match &metadata.resolution {
            LockfileResolution::Git(git) => read_git_build_environment(GitManifestQuery {
                source_cache: self.git_source_cache,
                repo: &git.repo,
                commit: &git.commit,
                path: git.path.as_deref(),
                git_shallow_hosts: &self.config.git_shallow_hosts,
                git_bin: None,
                git_config: &[],
            })
            .await
            .map_err(RecordBuildToolsError::ReadGitPackage),
            LockfileResolution::Tarball(tarball) => {
                let files = self.archive_files::<Reporter>(metadata_key, tarball).await?;
                archive_build_environment(metadata_key, &files, tarball.path.as_deref())
            }
            _ => Ok(None),
        }
    }

    async fn archive_files<Reporter: self::Reporter>(
        &self,
        metadata_key: &PackageKey,
        tarball: &TarballResolution,
    ) -> Result<Arc<HashMap<String, PathBuf>>, RecordBuildToolsError> {
        let config = self.config;
        let package_id = metadata_key.pkg_id();
        IngestTarballToStore {
            fetching: pnpm_tarball::ArchiveFetchOptions {
                http_client: self.http_client,
                auth_headers: &config.auth_headers,
                retry_opts: config.retry_opts(),
                offline: config.offline,
            },
            package: pnpm_tarball::TarballPackage {
                integrity: tarball.integrity.as_ref(),
                unpacked_size: None,
                file_count: None,
                url: &tarball.tarball,
                id: &package_id,
            },
            store: pnpm_tarball::ArchiveStoreContext {
                dir: &config.store_dir,
                index: None,
                index_writer: None,
                verify_integrity: config.verify_store_integrity,
                strict_pkg_content_check: config.strict_store_pkg_content_check,
                verified_files_cache: SharedVerifiedFilesCache::default(),
                prefetched_cas_paths: None,
            },
            requester: "",
            ignore_file_pattern: None,
            progress_reported: None,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: None,
            },
        }
        .run_with_mem_cache::<Reporter>(self.tarball_mem_cache)
        .await
        .map_err(RecordBuildToolsError::ReadArchive)
    }
}

/// The build environment of the package in the `dir` of a git host's
/// archive.
fn archive_build_environment(
    metadata_key: &PackageKey,
    files: &HashMap<String, PathBuf>,
    dir: Option<&str>,
) -> Result<Option<BuildEnvironment>, RecordBuildToolsError> {
    let repository = CasPackageFiles { files, dir: None };
    let package = CasPackageFiles { files, dir };
    let Some(manifest_path) = files.get(&archive_manifest_path(dir)) else { return Ok(None) };
    let manifest = std::fs::read(manifest_path)
        .map_err(|source| RecordBuildToolsError::ReadArchiveFile {
            package: metadata_key.to_string(),
            source,
        })?;
    let manifest = pnpm_package_manifest::parse_manifest_bytes(&manifest)
        .map_err(|source| RecordBuildToolsError::ReadManifest {
            package: metadata_key.to_string(),
            source,
        })?;
    Ok(detect_build_environment(&repository, &package, &manifest))
}

fn archive_manifest_path(dir: Option<&str>) -> String {
    match dir.unwrap_or("").trim_matches('/') {
        "" => "package.json".to_string(),
        dir => format!("{dir}/package.json"),
    }
}
