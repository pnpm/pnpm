//! A project packed for publishing: its archive that targets `any`, its
//! platform variants, and the manifests the publish scripts and notes read.

use super::{PackedPkg, PublishArgs};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    Host as PackHost, PackOptions, PackedArchives, WorkspacePackageManifest, pack_archives,
};
use pnpm_publish::{PackedVariant, StagedReference, VariantArchive};
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct PackedDirectory {
    pub(super) project_dir: PathBuf,
    pub(super) source_manifest: Value,
    /// The version's manifest: its `any` archive's, or the base manifest of
    /// a version published with variants only.
    pub(super) published_manifest: Value,
    /// The version's archive that targets `any`; `None` for a version
    /// published with variants only.
    pub(super) any: Option<PackedTarball>,
    /// The version's archive that targets `any`, staged by another machine.
    pub(super) any_staged: Option<StagedReference>,
    pub(super) notes: Option<String>,
    /// The platform variants published with the version.
    pub(super) variants: Vec<PackedVariant>,
}

/// A packed archive's bytes, where pack wrote it, and what it holds.
pub(super) struct PackedTarball {
    pub(super) data: Vec<u8>,
    pub(super) path: String,
    pub(super) contents: Vec<String>,
    pub(super) unpacked_size: u64,
}

impl PackedDirectory {
    pub(super) fn packed_pkg(&self) -> PackedPkg<'_> {
        let any = self.any.as_ref();
        PackedPkg {
            published_manifest: &self.published_manifest,
            tarball_data: any.map_or(&[], |any| &any.data),
            tarball_path: any.map_or("", |any| &any.path),
            contents: any.map_or(&[], |any| &any.contents),
            unpacked_size: any.map_or(0, |any| any.unpacked_size),
            notes: self.notes.as_deref(),
            variants: &self.variants,
            any_staged: self.any_staged.as_ref(),
        }
    }
}

/// Whether the project's manifest declares `publishConfig.variants`.
pub(super) fn declares_variants(manifest: &Value) -> bool {
    manifest.pointer("/publishConfig/variants").is_some()
}

/// The packed archives of the project at `project_dir`, read for publishing:
/// the archive that targets `any` as the version's, and every other one as a
/// platform variant. A version without an `any` archive is published with
/// the base manifest.
pub(super) fn packed_directory(
    project_dir: &Path,
    source_manifest: Value,
    packed: PackedArchives,
) -> miette::Result<PackedDirectory> {
    let mut any = None;
    let mut variants = Vec::new();
    for archive in packed.archives {
        match archive.entry.and_then(|entry| entry.tag) {
            Some(tag) => {
                let tarball_data = read_tarball(&archive.result.tarball_path)?;
                variants.push(PackedVariant { tag, archive: VariantArchive::Packed(tarball_data) });
            }
            None => any = Some(archive.result),
        }
    }
    let published_manifest = match &any {
        Some(result) => result.published_manifest.clone(),
        None => packed.base_manifest,
    };
    let any = any
        .map(|result| -> miette::Result<PackedTarball> {
            Ok(PackedTarball {
                data: read_tarball(&result.tarball_path)?,
                path: result.tarball_path,
                contents: result.contents,
                unpacked_size: result.unpacked_size,
            })
        })
        .transpose()?;
    Ok(PackedDirectory {
        project_dir: project_dir.to_path_buf(),
        source_manifest,
        published_manifest,
        any,
        any_staged: None,
        notes: None,
        variants,
    })
}

fn read_tarball(path: &str) -> miette::Result<Vec<u8>> {
    std::fs::read(path).into_diagnostic().wrap_err("read packed tarball")
}

/// The workspace's packages by name, shared across a recursive publish.
pub(super) type WorkspacePackages = Arc<HashMap<String, WorkspacePackageManifest>>;

impl PublishArgs {
    /// Pack the project into `pack_destination` for publishing (never a dry
    /// run; the publish itself honors `--dry-run`).
    pub(super) async fn pack_for_publish<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        pack_destination: &Path,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        (workspace_packages, targets): (Option<&WorkspacePackages>, &[String]),
    ) -> miette::Result<PackedArchives> {
        let options = self.publish_pack_options(
            dir,
            config,
            pack_destination,
            before_packing_hooks,
            workspace_packages,
        )
        .await?;
        pack_archives::<Reporter, PackHost>(&options, targets).await
            .map_err(miette::Report::new)
            .wrap_err(crate::cli_args::pack::PACK_ERROR_CONTEXT)
    }

    /// The pack options a publish packs the project at `dir` with, into
    /// `pack_destination` (never a dry run; the publish itself honors
    /// `--dry-run`).
    pub(super) async fn publish_pack_options(
        &self,
        dir: &Path,
        config: &Config,
        pack_destination: &Path,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        workspace_packages: Option<&WorkspacePackages>,
    ) -> miette::Result<PackOptions> {
        let workspace_packages = workspace_packages
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
            before_packing_hooks,
            workspace_packages,
        )?;
        let mut options = PackOptions {
            dir: dir.to_path_buf(),
            workspace_dir: config.workspace_dir.clone(),
            scripts: pnpm_pack::PackScripts {
                ignore: self.should_ignore_scripts(config),
                unsafe_perm: config.unsafe_perm,
                user_agent: config.user_agent.clone(),
                extra_bin_paths: config.extra_bin_paths.clone(),
                extra_env: config.extra_env.clone(),
            },
            manifest,
            output: pnpm_pack::PackOutputOptions {
                gzip_level: None,
                dry_run: false,
                out: None,
                destination: Some(pack_destination.to_string_lossy().into_owned()),
                injected_files: Vec::new(),
                locks: None,
            },
        };
        crate::cli_args::pack::set_injected_changelog(&mut options, config, dir).await?;
        Ok(options)
    }
}
