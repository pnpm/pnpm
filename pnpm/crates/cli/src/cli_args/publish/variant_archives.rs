//! Publishing a version's archives from several machines: each uploads the
//! archives it built with `--upload-variant`, and a final publish references
//! them with `--variant-integrity`.

use super::{
    PublishArgs, PublishScope, given_archives, packed::PackedDirectory, recursive::publish_eligible,
};
use crate::cli_args::registry_client::build_publish_client;
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{PackedArchive, PackedArchives, variants::VariantDeclaration};
use pnpm_publish::{
    Host, PublishNetwork, UploadedArchive, resolve_otp_from_env, upload_variant_archives,
};
use pnpm_reporter::Reporter;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

impl PublishArgs {
    /// `--upload-variant`: pack the archives the targets name, upload them to
    /// the registry's staging area, and print `<target>=<integrity>` for each,
    /// the value a final publish takes with `--variant-integrity`. With
    /// `--json`, print the package, and each archive's tag, targets, and
    /// integrity.
    pub(super) async fn upload_variants<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    ) -> miette::Result<()> {
        if recursive || !self.flags.variants.variant_integrity.is_empty() {
            return Err(miette::miette!(
                code = "ERR_PNPM_UPLOAD_VARIANT_ALONE",
                "--upload-variant uploads one package's archives; run it in the package's directory, without --recursive or --variant-integrity",
            ));
        }
        let (_pack_destination, packed, bytes) =
            self.pack_uploads::<Reporter>(dir, config, before_packing_hooks).await?;
        let uploads: Vec<UploadedArchive<'_>> = bytes
            .iter()
            .map(|data| UploadedArchive::new(data))
            .collect();

        let otp = resolve_otp_from_env::<Host>(self.flags.registry.otp.clone());
        let opts = self.publish_options(config, otp, false);
        let http_client = build_publish_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };
        upload_variant_archives::<Host, Reporter>(&packed.base_manifest, &uploads, &opts, &network)
            .await
            .map_err(miette::Report::new)?;
        let printed = self.upload_output(&packed.base_manifest, &packed.archives, &uploads);
        pnpm_diagnostics::println!("{printed}");
        Ok(())
    }

    /// Pack the archives `--upload-variant` names into a temporary
    /// directory, kept alive by the first value, and read them.
    async fn pack_uploads<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    ) -> miette::Result<(tempfile::TempDir, PackedArchives, Vec<Vec<u8>>)> {
        let project_dir = self.package
            .as_deref()
            .map_or_else(|| dir.to_path_buf(), |path| dir.join(path));
        let pack_destination = tempfile::tempdir().into_diagnostic().wrap_err("create temp dir")?;
        let packed = self.pack_for_publish::<Reporter>(
            &project_dir,
            config,
            pack_destination.path(),
            before_packing_hooks,
            (None, &self.flags.variants.upload_variant),
        )
        .await?;
        let bytes = packed.archives
            .iter()
            .map(|archive| {
                std::fs::read(&archive.result.tarball_path)
                    .into_diagnostic()
                    .wrap_err("read packed tarball")
            })
            .collect::<miette::Result<Vec<_>>>()?;
        Ok((pack_destination, packed, bytes))
    }

    /// What `--upload-variant` prints: a `<target>=<integrity>` line per
    /// archive, with the target it was given, or the JSON summary.
    fn upload_output(
        &self,
        manifest: &Value,
        archives: &[PackedArchive],
        uploads: &[UploadedArchive<'_>],
    ) -> String {
        let named = |archive: &PackedArchive| {
            let targets = archive.entry
                .as_ref()
                .map(|entry| entry.targets.clone())
                .unwrap_or_default();
            let given = self.flags.variants.upload_variant
                .iter()
                .find(|target| targets.contains(target))
                .cloned()
                .unwrap_or_default();
            (given, targets)
        };
        if self.flags.output.json {
            let archives: Vec<Value> = archives
                .iter()
                .zip(uploads)
                .map(|(archive, upload)| {
                    let tag = archive.entry.as_ref().and_then(|entry| entry.tag.clone());
                    json!({ "tag": tag, "targets": named(archive).1, "integrity": upload.integrity })
                })
                .collect();
            let summary = json!({
                "name": manifest.get("name"),
                "version": manifest.get("version"),
                "archives": archives,
            });
            return serde_json::to_string_pretty(&summary).expect("the upload summary serializes");
        }
        archives
            .iter()
            .zip(uploads)
            .map(|(archive, upload)| format!("{}={}", named(archive).0, upload.integrity))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The project at `project_dir` published from the archives other
    /// machines staged, as `--variant-integrity` gives them, without packing
    /// any; `None` when no value names this package's archives, which a
    /// recursive publish packs as usual.
    pub(super) async fn given_directory<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        source_manifest: &Value,
        (before_packing_hooks, scope): (&[Arc<dyn PnpmfileHooks>], PublishScope<'_>),
    ) -> miette::Result<Option<PackedDirectory>> {
        if self.flags.variants.variant_integrity.is_empty() {
            return Ok(None);
        }
        let given = given_archives::parse(&self.flags.variants.variant_integrity)?;
        // A recursive publish skips a package no value names before reading
        // its manifest, by the name it publishes under.
        if matches!(scope, PublishScope::Recursive { .. }) && !names_any(&given, source_manifest) {
            return Ok(None);
        }
        let (base_manifest, declaration) =
            self.declared_manifests::<Reporter>(project_dir, config, (before_packing_hooks, scope))
                .await?;
        // Values name the package as it is published, which `publishConfig`
        // may rename, and the archives that manifest declares.
        let name = base_manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let found = declaration
            .as_ref()
            .map(|declaration| given_archives::archives_of(&name, declaration, &given))
            .transpose()?
            .flatten();
        // A value names this package, so the package publishes from staged
        // archives or not at all, never from archives packed here.
        let (Some(declaration), Some((any, variants))) = (declaration, found) else {
            return Err(miette::miette!(
                code = "ERR_PNPM_UNKNOWN_VARIANT",
                "--variant-integrity names no archive that {name} declares in publishConfig.variants",
            ));
        };
        Ok(Some(staged_directory(
            (project_dir, source_manifest),
            (&base_manifest, &declaration),
            (any, variants),
        )))
    }
}

impl PublishArgs {
    /// The manifest the project at `project_dir` publishes, without its
    /// `publishConfig.variants`, and that declaration, read without packing.
    async fn declared_manifests<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        (before_packing_hooks, scope): (&[Arc<dyn PnpmfileHooks>], PublishScope<'_>),
    ) -> miette::Result<(Value, Option<VariantDeclaration>)> {
        let pack_destination = tempfile::tempdir().into_diagnostic().wrap_err("create temp dir")?;
        let mut options = self.publish_pack_options(
            project_dir,
            config,
            pack_destination.path(),
            before_packing_hooks,
            scope.workspace_packages(),
        )
        .await?;
        // Nothing is packed, so the pack scripts have nothing to prepare.
        options.scripts.ignore = true;
        pnpm_pack::declared_manifests::<Reporter>(&options).await.map_err(miette::Report::new)
    }

    /// Check `--variant-integrity` against the projects a recursive publish
    /// selects, before any is packed (see [`given_archives::check_recursive`]).
    pub(super) fn check_recursive_variant_integrities(
        &self,
        graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
    ) -> miette::Result<()> {
        if self.flags.variants.variant_integrity.is_empty() {
            return Ok(());
        }
        let given = given_archives::parse(&self.flags.variants.variant_integrity)?;
        let packages: Vec<&str> = graph
            .iter()
            .filter_map(|(_, node)| publish_eligible(node.package.project.manifest.value()))
            .map(|(name, _)| name)
            .collect();
        given_archives::check_recursive(&given, &packages)
    }
}

/// The project at `project_dir`, published from the archives other machines
/// staged: its `any` archive, if it has one, and its `variants`.
fn staged_directory(
    (project_dir, source_manifest): (&Path, &Value),
    (base_manifest, declaration): (&Value, &VariantDeclaration),
    (any, variants): (Option<pnpm_publish::StagedReference>, Vec<pnpm_publish::PackedVariant>),
) -> PackedDirectory {
    let published_manifest = match declaration.primary() {
        Some(entry) => VariantDeclaration::archive_manifest(base_manifest, entry),
        None => base_manifest.clone(),
    };
    PackedDirectory {
        project_dir: project_dir.to_path_buf(),
        source_manifest: source_manifest.clone(),
        published_manifest,
        any: None,
        any_staged: any,
        notes: None,
        variants,
    }
}

/// Whether a value of `given` names, by its package, the project whose own
/// manifest is `manifest`.
fn names_any(given: &[given_archives::GivenIntegrity], manifest: &Value) -> bool {
    let name = publish_eligible(manifest).map(|(name, _)| name);
    given
        .iter()
        .any(|value| {
            value.package
                .as_deref()
                .is_some_and(|package| Some(package) == name)
        })
}
