//! A project directory packed for publishing.

use super::{
    PublishArgs,
    PublishScope,
    run_publish_scripts,
};
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    Host as PackHost,
    PackOptions,
    PackResult,
    api as pack_api,
};
use pnpm_publish::PackedPkg;
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::{
    path::Path,
    sync::Arc,
};

pub(super) struct PackedDirectory {
    pub(super) project_dir: std::path::PathBuf,
    pub(super) source_manifest: Value,
    pub(super) published_manifest: Value,
    pub(super) tarball_data: Vec<u8>,
    pub(super) tarball_path: String,
    pub(super) contents: Vec<String>,
    pub(super) unpacked_size: u64,
    pub(super) notes: Option<String>,
}

impl PackedDirectory {
    pub(super) fn packed_pkg(&self) -> PackedPkg<'_> {
        PackedPkg {
            published_manifest: &self.published_manifest,
            tarball_data: &self.tarball_data,
            tarball_path: &self.tarball_path,
            contents: &self.contents,
            unpacked_size: self.unpacked_size,
            notes: self.notes.as_deref(),
        }
    }
}

impl PublishArgs {
    pub(super) async fn pack_directory<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        scope: PublishScope<'_>,
    ) -> miette::Result<PackedDirectory> {
        let manifest = pnpm_package_manifest::safe_read_project_manifest_from_dir(project_dir)
            .into_diagnostic()
            .wrap_err("read project manifest")?
            .ok_or_else(|| {
                let dir = project_dir.display();
                miette::miette!(
                    code = "ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND",
                    "No package.json found in {dir}",
                )
            })?;

        if !self.should_ignore_scripts(config) {
            run_publish_scripts::<Reporter>(
                project_dir,
                config,
                &manifest,
                &["prepublishOnly", "prepublish"],
            )?;
        }

        let pack_destination = tempfile::tempdir().into_diagnostic().wrap_err("create temp dir")?;
        let pack_result = self.pack_for_publish::<Reporter>(
            project_dir,
            config,
            pack_destination.path(),
            before_packing_hooks,
            scope,
        )
        .await?;
        let tarball_data = std::fs::read(&pack_result.tarball_path)
            .into_diagnostic()
            .wrap_err("read packed tarball")?;

        Ok(PackedDirectory {
            project_dir: project_dir.to_path_buf(),
            source_manifest: manifest,
            published_manifest: pack_result.published_manifest,
            tarball_data,
            tarball_path: pack_result.tarball_path,
            contents: pack_result.contents,
            unpacked_size: pack_result.unpacked_size,
            notes: None,
        })
    }

    fn pack_scripts(&self, config: &Config) -> pnpm_pack::PackScripts {
        pnpm_pack::PackScripts {
            ignore: self.should_ignore_scripts(config),
            unsafe_perm: config.unsafe_perm,
            user_agent: config.user_agent.clone(),
            extra_bin_paths: config.extra_bin_paths.clone(),
            extra_env: config.extra_env.clone(),
        }
    }

    /// Pack the project into `pack_destination` for publishing (never a dry
    /// run; the publish itself honors `--dry-run`).
    pub(super) async fn pack_for_publish<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        pack_destination: &Path,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        scope: PublishScope<'_>,
    ) -> miette::Result<PackResult> {
        let workspace_packages = scope
            .workspace_packages()
            .cloned()
            .or_else(|| {
                crate::cli_args::workspace_packages::discover_workspace_package_manifests(
                    config.workspace_dir.as_deref(),
                    config,
                )
            });
        let manifest = crate::cli_args::workspace_packages::create_publish_pack_manifest_options(
            &self.flags.manifest,
            config,
            before_packing_hooks.to_vec(),
            workspace_packages,
            scope.bumped(),
        )?;
        let mut options = PackOptions {
            dir: dir.to_path_buf(),
            workspace_dir: config.workspace_dir.clone(),
            scripts: self.pack_scripts(config),
            manifest,
            output: pnpm_pack::PackOutputOptions {
                gzip_level: None,
                dry_run: false,
                base_dir: None,
                out: None,
                destination: Some(pack_destination.to_string_lossy().into_owned()),
                injected_files: Vec::new(),
                locks: None,
            },
        };
        crate::cli_args::pack::set_injected_changelog(&mut options, config, dir).await?;
        pack_api::<Reporter, PackHost>(&options).await
            .map_err(miette::Report::new)
            .wrap_err(crate::cli_args::pack::PACK_ERROR_CONTEXT)
    }
}
