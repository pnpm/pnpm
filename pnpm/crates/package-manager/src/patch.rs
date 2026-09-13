pub use candidates::{default_patch_target, patch_candidates_from_lockfile};
mod candidates;

use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, InstallPackageBySnapshotError, import_indexed_dir,
    retry_config::retry_opts_from_config, tarball_url_and_integrity,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, PackageImportMethod};
use pnpm_deps_restorer::build_modules::exec_scripts_prepend_node_path;
use pnpm_git_fetcher::{GitFetchOutput, GitFetcherError, GitHostedTarballFetcher};
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_network::ThrottledClient;
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    SharedVerifiedFilesCache, StoreIndex, StoreIndexError, StoreIndexWriter,
    git_hosted_store_index_key,
};
use pnpm_tarball::{IngestTarballToStore, MemCache, TarballError};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

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
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_PATCH_PACKAGE_MISSING_METADATA))]
    MissingPackageMetadata { package_key: String },

    #[display(
        "Package `{package}` uses a `{resolution_kind}` resolution, which `pnpm patch` does not yet support."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_PATCH_UNSUPPORTED_RESOLUTION))]
    UnsupportedResolution {
        package: String,
        resolution_kind: &'static str,
    },

    #[diagnostic(transparent)]
    TarballResolution(#[error(source)] InstallPackageBySnapshotError),

    #[display("Failed to inspect patch edit directory {dest:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_PATCH_DEST_STAT))]
    PatchDestinationStat {
        dest: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Refusing to write package files to unsafe patch edit directory {dest:?}: {reason}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UNSAFE_PATCH_DEST))]
    UnsafePatchDestination { dest: PathBuf, reason: &'static str },

    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    GitFetch(#[error(source)] GitFetcherError),

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),
}

impl WritePackageForPatch<'_> {
    pub async fn run<Reporter: self::Reporter>(self) -> Result<(), WritePackageForPatchError> {
        self.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let metadata = self.current_lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&self.target.package_key))
            .ok_or_else(|| WritePackageForPatchError::MissingPackageMetadata {
                package_key: self.target.package_key.to_string(),
            })?;

        if !matches!(
            metadata.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            return Err(WritePackageForPatchError::UnsupportedResolution {
                package: format!("{}@{}", self.target.alias, self.target.version),
                resolution_kind: resolution_kind(&metadata.resolution),
            });
        }

        let (tarball_url, integrity) =
            tarball_url_and_integrity(&metadata.resolution, &self.target.package_key, self.config)
                .map_err(WritePackageForPatchError::TarballResolution)?;
        let package_id = self.target.package_key.pkg_id();

        validate_patch_destination(self.dest)?;

        let store_index = StoreIndex::open_shared(&self.config.store_dir, self.config.frozen_store)
            .await;
        let (store_index_writer, writer_task) =
            StoreIndexWriter::spawn_for(&self.config.store_dir, self.config.frozen_store);

        let result = self.import_for_patch::<Reporter>(
            &metadata.resolution,
            &tarball_url,
            integrity,
            &package_id,
            store_index,
            &store_index_writer,
        )
        .await;

        shutdown_store_index_writer_for_patch(store_index_writer, writer_task)
            .await;
        result
    }
    async fn import_for_patch<Reporter: self::Reporter>(
        &self,
        resolution: &LockfileResolution,
        tarball_url: &str,
        integrity: Option<&ssri::Integrity>,
        package_id: &str,
        store_index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
        store_index_writer: &Arc<StoreIndexWriter>,
    ) -> Result<(), WritePackageForPatchError> {
        let cas_paths = self.download_for_patch::<Reporter>(
            tarball_url,
            integrity,
            package_id,
            store_index,
            store_index_writer,
        )
        .await?;
        let cas_paths = git_hosted_cas_paths::<Reporter>(
            self.config,
            resolution,
            (*cas_paths).clone(),
            package_id,
            store_index_writer,
        )
        .await?;
        import_indexed_dir::<Reporter>(
            &AtomicU8::new(0),
            PackageImportMethod::CloneOrCopy,
            self.dest,
            &cas_paths,
            ImportIndexedDirOpts {
                force: true,
                ..ImportIndexedDirOpts::default()
            },
        )
        .map_err(WritePackageForPatchError::ImportIndexedDir)
    }
    async fn download_for_patch<Reporter: self::Reporter>(
        &self,
        tarball_url: &str,
        integrity: Option<&ssri::Integrity>,
        package_id: &str,
        store_index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
        store_index_writer: &Arc<StoreIndexWriter>,
    ) -> Result<Arc<std::collections::HashMap<String, PathBuf>>, WritePackageForPatchError> {
        IngestTarballToStore {
            fetching: pnpm_tarball::ArchiveFetchOptions {
                http_client: self.http_client,
                auth_headers: &self.config.auth_headers,
                retry_opts: retry_opts_from_config(self.config),
                offline: self.config.offline,
            },
            package: pnpm_tarball::TarballPackage {
                integrity,
                unpacked_size: None,
                file_count: None,
                url: tarball_url,
                id: package_id,
            },
            store: pnpm_tarball::ArchiveStoreContext {
                dir: &self.config.store_dir,
                index: store_index,
                index_writer: Some(Arc::clone(store_index_writer)),
                verify_integrity: self.config.verify_store_integrity,
                strict_pkg_content_check: self.config
                    .strict_store_pkg_content_check,
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
        .map_err(WritePackageForPatchError::DownloadTarball)
    }
}

/// The tarball's files, re-fetched through the git-hosted fetcher when the
/// resolution is a git-hosted archive, whose subdirectory and prepare step
/// the plain ingest does not apply.
async fn git_hosted_cas_paths<Reporter: self::Reporter>(
    config: &Config,
    resolution: &LockfileResolution,
    cas_paths: std::collections::HashMap<String, std::path::PathBuf>,
    package_id: &str,
    store_index_writer: &Arc<StoreIndexWriter>,
) -> Result<std::collections::HashMap<String, std::path::PathBuf>, WritePackageForPatchError> {
    let LockfileResolution::Tarball(tarball) = resolution else {
        return Ok(cas_paths);
    };
    if git_tarball_url(resolution).is_none() {
        return Ok(cas_paths);
    }
    let allow_build_closure = |_dep_path: &str| false;
    let files_index_file = git_hosted_store_index_key(package_id, !config.ignore_scripts);
    let GitFetchOutput { cas_paths, built: _built } = GitHostedTarballFetcher {
        scripts: pnpm_git_fetcher::PrepareScriptOptions {
            ignore: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: Some(&config.user_agent),
            prepend_node_path: exec_scripts_prepend_node_path(config),
            shell: None,
            node_execpath: None,
            npm_execpath: None,
            pnpm_execpath: None,
        },
        store: pnpm_git_fetcher::GitStoreContext {
            dir: &config.store_dir,
            index_writer: Some(store_index_writer),
            files_index_file: &files_index_file,
        },
        cas_paths,
        path: tarball.path.as_deref(),
        allow_build: &allow_build_closure,

        // Nothing here is allowed to build, so no package
        // manager has to be provided to it.
        package_id,
        requester: "",
    }
    .run::<Reporter>()
    .await
    .map_err(WritePackageForPatchError::GitFetch)?;
    Ok(cas_paths)
}

fn validate_patch_destination(dest: &Path) -> Result<(), WritePackageForPatchError> {
    match std::fs::symlink_metadata(dest) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(WritePackageForPatchError::UnsafePatchDestination {
                dest: dest.to_path_buf(),
                reason: "destination is a symlink",
            })
        }
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(WritePackageForPatchError::PatchDestinationStat {
            dest: dest.to_path_buf(),
            source,
        }),
    }
}

async fn shutdown_store_index_writer_for_patch(
    store_index_writer: Arc<StoreIndexWriter>,
    writer_task: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
) {
    drop(store_index_writer);
    StoreIndexWriter::drain(writer_task, "; some rows may not be persisted")
        .await;
}

fn git_tarball_url(resolution: &LockfileResolution) -> Option<String> {
    let LockfileResolution::Tarball(tarball) = resolution else {
        return None;
    };
    (tarball.is_git_hosted() || tarball.tarball.starts_with("https://pkg.pr.new/")).then(|| {
        tarball.tarball.clone()
    })
}

fn resolution_kind(resolution: &LockfileResolution) -> &'static str {
    match resolution {
        LockfileResolution::Tarball(_) => "tarball",
        LockfileResolution::Registry(_) => "registry",
        LockfileResolution::Directory(_) => "directory",
        LockfileResolution::Git(_) => "git",
        LockfileResolution::Binary(_) => "binary",
        LockfileResolution::Variations(_) => "variations",
        LockfileResolution::Custom(_) => "custom",
    }
}

#[cfg(test)]
mod tests;
