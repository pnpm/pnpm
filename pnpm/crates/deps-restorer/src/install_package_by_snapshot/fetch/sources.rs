use super::{
    super::{
        InstallPackageBySnapshot, InstallPackageBySnapshotError, PNPM_EXECPATH,
        runtime::{archive_filter_for, fetch_binary_resolution_to_cas},
        tarball_resolution::{local_file_tarball_install_url, tarball_url_and_integrity},
    },
    CustomFetched, SnapshotFetch, TarballFetch, download_tarball,
};
use crate::{build_modules::exec_scripts_prepend_node_path, custom_fetcher::CustomFetchOutcome};
use pnpm_git_fetcher::{GitFetchOutput, GitFetcher, GitHostedTarballFetcher};
use pnpm_lockfile::{BinaryResolution, LockfileResolution, PackageKey, PackageMetadata};
use pnpm_reporter::Reporter;
use pnpm_store_dir::git_hosted_store_index_key;
use pnpm_tarball::IngestTarballToStore;
use std::{collections::HashMap, path::PathBuf};

impl InstallPackageBySnapshot<'_> {
    pub(in super::super) async fn custom_fetch<Reporter: self::Reporter>(
        &self,
        package_key: &PackageKey,
        metadata: &PackageMetadata,
        package_id: &str,
        download: &IngestTarballToStore<'_>,
    ) -> Result<CustomFetched, InstallPackageBySnapshotError> {
        let Some(session) = self.custom_fetcher_session else {
            return Ok(CustomFetched { resolution: None, cas_paths: None });
        };
        let config = self.ctx.config;
        let opts = serde_json::json!({
            "pkg": {
                "name": package_key.name.to_string(),
                "version": metadata.version.clone()
                    .unwrap_or_else(|| package_key.suffix.version().to_string()),
            },
            "lockfileDir": self.ctx.workspace_root,
            "readManifest": true,
            "filesIndexFile": pnpm_store_dir::pick_store_index_key(
                metadata.resolution.checkable_integrity().map(ToString::to_string).as_deref(),
                false, package_id, !config.ignore_scripts,
            ),
        });
        Ok(match session.fetch::<Reporter>(download.clone(), &metadata.resolution, opts).await? {
            CustomFetchOutcome::Declined(resolution)
            | CustomFetchOutcome::Delegate { delegate: resolution, .. } => {
                CustomFetched { resolution: Some(resolution), cas_paths: None }
            }
            CustomFetchOutcome::Fetched { tarball, .. } => {
                CustomFetched { resolution: None, cas_paths: Some(tarball.files_map.clone()) }
            }
        })
    }
    pub(in super::super) async fn fetch_binary<Reporter: self::Reporter>(
        &self,
        binary: &BinaryResolution,
        package_key: &PackageKey,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        fetch_binary_resolution_to_cas::<Reporter>(
            binary,
            self.http_client,
            self.ctx.config,
            self.store_index,
            self.store_index_writer,
            self.verified_files_cache,
            self.prefetched_cas_paths,
            package_key,
            self.ctx.requester,
            archive_filter_for(package_key),
        )
        .await
    }
    pub(in super::super) async fn fetch_git<Reporter: self::Reporter>(
        &self,
        fetch: &SnapshotFetch<'_>,
        git_resolution: &pnpm_lockfile::GitResolution,
        allow_build: &(impl Fn(&str) -> bool + Send + Sync),
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        // Same `built = !ignore_scripts` rationale as the git-hosted
        // tarball branch — key shape stays in lock-step with
        // `snapshot_cache_key`.
        let built = !config.ignore_scripts;
        let files_index_file = git_hosted_store_index_key(fetch.package_id, built);
        let package_name = fetch.package_key.name.to_string();
        let GitFetchOutput { cas_paths, built: _built } = GitFetcher {
            source_cache: self.ctx.git_source_cache,
            repo: &git_resolution.repo,
            commit: &git_resolution.commit,
            path: git_resolution.path.as_deref(),
            git_shallow_hosts: &config.git_shallow_hosts,
            allow_build,
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: Some(&config.user_agent),
            scripts_prepend_node_path: exec_scripts_prepend_node_path(config),
            script_shell: None,
            node_execpath: None,
            npm_execpath: None,
            pnpm_execpath: PNPM_EXECPATH.as_deref(),
            store_dir: &config.store_dir,
            package_id: fetch.package_id,
            package_name: &package_name,
            requester: self.ctx.requester,
            store_index_writer: self.store_index_writer,
            files_index_file: &files_index_file,
            git_bin: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::GitFetch)?;
        Ok(cas_paths)
    }
}
impl InstallPackageBySnapshot<'_> {
    /// Fetch a tarball- or registry-shaped resolution into the store and
    /// return its CAS paths.
    pub(super) async fn tarball_cas_paths<Reporter: self::Reporter>(
        &self,
        fetch: TarballFetch<'_, impl Fn(&str) -> bool + Send + Sync>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        let revision_addressed = match fetch.resolution {
            LockfileResolution::Tarball(tarball) => tarball.revision.is_some(),
            LockfileResolution::Registry(registry) => registry.revision.is_some(),
            _ => false,
        };
        let (tarball_url, integrity) =
            tarball_url_and_integrity(fetch.resolution, fetch.package_key, config)?;
        let tarball_url = local_file_tarball_install_url(tarball_url, self.ctx.workspace_root);
        let download = IngestTarballToStore {
            package_url: &tarball_url,
            package_integrity: integrity,
            ..fetch.download.clone()
        };
        let raw_cas_paths = download_tarball::<Reporter>(
            download,
            self.tarball_mem_cache
                .filter(|_| matches!(fetch.resolution, LockfileResolution::Registry(_)))
                .map(std::convert::AsRef::as_ref),
            revision_addressed,
        )
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?;
        match fetch.resolution {
            LockfileResolution::Tarball(tarball) if tarball.is_git_hosted() => {
                self.prepare_git_hosted::<Reporter>(&fetch, tarball, raw_cas_paths).await
            }
            _ => Ok(raw_cas_paths),
        }
    }

    /// The git-hosted prepare+packlist pass: a `gitHosted: true` tarball
    /// routes through `gitHostedTarballFetcher` rather than the plain
    /// `remoteTarballFetcher`, because the host's archive endpoint
    /// doesn't run `prepare`/`prepublish*` and the file set typically
    /// needs packlist filtering.
    async fn prepare_git_hosted<Reporter: self::Reporter>(
        &self,
        fetch: &TarballFetch<'_, impl Fn(&str) -> bool + Send + Sync>,
        tarball: &pnpm_lockfile::TarballResolution,
        cas_paths: HashMap<String, PathBuf>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        // `built` tracks `!ignore_scripts`, in lock-step with the key
        // shape `snapshot_cache_key` produces — otherwise the prefetch
        // and the write would address different slots. Under
        // `--ignore-scripts` the git-hosted `prepare` is suppressed too,
        // matching pnpm's `ignoreScripts`.
        let files_index_file = git_hosted_store_index_key(fetch.package_id, !config.ignore_scripts);
        let GitFetchOutput { cas_paths, built: _built } = GitHostedTarballFetcher {
            cas_paths,
            path: tarball.path.as_deref(),
            allow_build: fetch.allow_build,
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: Some(&config.user_agent),
            scripts_prepend_node_path: fetch.scripts_prepend_node_path,
            script_shell: None,
            node_execpath: None,
            npm_execpath: None,
            pnpm_execpath: PNPM_EXECPATH.as_deref(),
            store_dir: &config.store_dir,
            package_id: fetch.package_id,
            requester: self.ctx.requester,
            store_index_writer: self.store_index_writer,
            files_index_file: &files_index_file,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::GitFetch)?;
        Ok(cas_paths)
    }
}
