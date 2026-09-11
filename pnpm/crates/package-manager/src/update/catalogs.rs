use super::UpdateError;
use crate::{CatalogDecision, CatalogModeDep, decide_catalog};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{CatalogMode, Config};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use std::path::PathBuf;

/// Route each rewrite through the catalog mode, returning the workspace
/// directory whose manifest holds the catalogs when any were consulted.
pub(super) fn reconcile_catalog_rewrites<Reporter: self::Reporter>(
    manifest: &PackageManifest,
    config: &Config,
    latest: bool,
    direct: &[(String, DependencyGroup, String)],
    rewrites: &mut Vec<(String, DependencyGroup, String)>,
    catalog_ctx: &mut Option<CatalogCtx>,
    updated_catalogs: &mut Catalogs,
) -> Result<Option<PathBuf>, UpdateError> {
    if rewrites.is_empty() || (config.catalog_mode == CatalogMode::Manual && catalog_ctx.is_none())
    {
        return Ok(None);
    }
    let ctx = ensure_catalog_ctx(catalog_ctx, manifest, config)?;
    let mut reconciled = Vec::with_capacity(rewrites.len());
    for (name, group, specifier) in std::mem::take(rewrites) {
        let previous = direct
            .iter()
            .find(|(previous_name, previous_group, _)| {
                *previous_name == name && *previous_group == group
            })
            .map(|(_, _, previous_specifier)| previous_specifier.as_str());
        let reconciliation = reconcile_rewrite::<Reporter>(
            config,
            ctx,
            latest,
            updated_catalogs,
            (&name, &specifier, previous),
        )?;
        if let Some(specifier) = reconciliation {
            reconciled.push((name, group, specifier));
        }
    }
    *rewrites = reconciled;
    Ok(ctx.workspace_dir_opt.clone().or_else(|| Some(ctx.manifest_dir.clone())))
}
/// The specifier one rewrite records in the manifest, or `None` when the
/// catalog mode moved it into a catalog instead.
pub(super) fn reconcile_rewrite<Reporter: self::Reporter>(
    config: &Config,
    ctx: &CatalogCtx,
    latest: bool,
    updated_catalogs: &mut Catalogs,
    rewrite: (&str, &str, Option<&str>),
) -> Result<Option<String>, UpdateError> {
    let (name, specifier, previous) = rewrite;
    if latest && let Some(catalog_name) = previous.and_then(parse_catalog_protocol) {
        updated_catalogs
            .entry(catalog_name.to_string())
            .or_default()
            .insert(name.to_string(), specifier.to_string());
        return Ok(None);
    }
    if config.catalog_mode == CatalogMode::Manual {
        return Ok(Some(specifier.to_string()));
    }
    let dependency =
        CatalogModeDep { alias: name, bare_specifier: specifier, prev_specifier: previous };
    let decision = decide_catalog::<Reporter>(
        config.catalog_mode,
        None,
        &ctx.catalogs,
        &dependency,
        &ctx.prefix,
    )
    .map_err(UpdateError::CatalogVersionMismatch)?;
    match decision {
        CatalogDecision::KeepDirect => Ok(Some(specifier.to_string())),
        CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
            if let Some(entry) = updated_entry {
                updated_catalogs
                    .entry(entry.catalog_name)
                    .or_default()
                    .insert(name.to_string(), entry.specifier);
            }
            Ok(Some(manifest_specifier))
        }
    }
}
pub(super) fn merge_catalogs(target: &mut Catalogs, updates: &Catalogs) {
    for (catalog_name, entries) in updates {
        let catalog = target.entry(catalog_name.clone()).or_default();
        for (dependency, specifier) in entries {
            catalog.insert(dependency.clone(), specifier.clone());
        }
    }
}
/// The workspace catalogs and the directories needed to read the existing
/// `catalog:` entries (to preserve their range operators) and write the
/// bumped ones back to `pnpm-workspace.yaml`.
pub(super) struct CatalogCtx {
    pub(super) catalogs: Catalogs,
    /// The workspace root, or `None` when the project is not part of a
    /// workspace (entries are then written next to `package.json`).
    workspace_dir_opt: Option<std::path::PathBuf>,
    manifest_dir: std::path::PathBuf,
    /// Workspace (or project) directory as a string, for warning messages.
    prefix: String,
}
/// Borrow the effective catalogs, reading them on first use.
pub(super) fn ensure_catalog_ctx<'slot>(
    slot: &'slot mut Option<CatalogCtx>,
    manifest: &PackageManifest,
    config: &Config,
) -> Result<&'slot CatalogCtx, UpdateError> {
    if slot.is_none() {
        *slot = Some(read_catalog_ctx(manifest, config)?);
    }
    Ok(slot.as_ref().expect("just populated"))
}
pub(super) fn effective_specifier(
    catalog_ctx: &mut Option<CatalogCtx>,
    manifest: &PackageManifest,
    config: &Config,
    prev: &str,
    name: &str,
) -> Result<String, UpdateError> {
    if let Some(catalog_name) = parse_catalog_protocol(prev) {
        let ctx = ensure_catalog_ctx(catalog_ctx, manifest, config)?;
        if let Some(spec) = ctx.catalogs.get(catalog_name).and_then(|catalog| catalog.get(name)) {
            return Ok(spec.clone());
        }
    }
    Ok(prev.to_string())
}
/// Read the effective catalogs and the directories around them.
///
/// The catalogs prefer a post-`updateConfig` pnpmfile hook's output
/// (`config.catalogs`, the authoritative complete set) over the raw
/// `pnpm-workspace.yaml` read, matching `Install::run` so an update never
/// resolves `catalog:` deps against stale on-disk catalogs when a hook
/// changed them. Workspace discovery still drives where bumped entries are
/// written back.
pub(super) fn read_catalog_ctx(
    manifest: &PackageManifest,
    config: &Config,
) -> Result<CatalogCtx, UpdateError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        crate::install::configured_or_discovered_workspace_dir(config, &manifest_dir)
            .map_err(UpdateError::FindWorkspaceDir)?;
    let catalogs = if let Some(catalogs) = config.catalogs.clone() {
        catalogs
    } else {
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pnpm_workspace::read_workspace_manifest(dir)
                .map_err(UpdateError::ReadWorkspaceManifest)?,
            None => None,
        };
        get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
            .map_err(UpdateError::InvalidCatalogsConfiguration)?
    };
    let prefix =
        workspace_dir_opt.as_deref().unwrap_or(&manifest_dir).to_string_lossy().into_owned();
    Ok(CatalogCtx { catalogs, workspace_dir_opt, manifest_dir, prefix })
}
pub(super) fn read_catalog_ctx_with_catalogs(
    manifest: &PackageManifest,
    config: &Config,
    catalogs: Catalogs,
) -> Result<CatalogCtx, UpdateError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        crate::install::configured_or_discovered_workspace_dir(config, &manifest_dir)
            .map_err(UpdateError::FindWorkspaceDir)?;
    let prefix =
        workspace_dir_opt.as_deref().unwrap_or(&manifest_dir).to_string_lossy().into_owned();
    Ok(CatalogCtx { catalogs, workspace_dir_opt, manifest_dir, prefix })
}
