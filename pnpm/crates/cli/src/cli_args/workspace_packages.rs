use crate::cli_args::{install::resolve_bool_override, publish::PublishManifestArgs};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_lockfile::DeclaredRegistryPrefixes;
use pnpm_pack::{PackManifestOptions, WorkspacePackageManifest, WorkspacePackageManifests};
use pnpm_package_manifest::{PackageManifestError, safe_read_project_manifest_from_dir};
use pnpm_publish::{find_registry_info, publish_config_registry};
use pnpm_workspace::Project;
use serde_json::Value;
use std::{borrow::Cow, path::Path, sync::Arc};

/// The workspace's packages by name, one entry per project, in discovery
/// order.
pub fn build_workspace_package_manifest_map(
    projects: &[Project],
    config: &Config,
) -> Result<WorkspacePackageManifests, PackageManifestError> {
    let declared_prefixes = config.workspace_declared_registry_prefixes();
    let mut map = WorkspacePackageManifests::new();
    for project in projects {
        let manifest = project.manifest.value();
        let Some(name) = manifest.get("name").and_then(Value::as_str) else {
            continue;
        };
        map.entry(name.to_string())
            .or_default()
            .push(WorkspacePackageManifest {
                name: name.to_string(),
                version: manifest
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                root_dir: Some(project.root_dir.clone()),
                registry_prefix: publish_registry_prefix(
                    project,
                    name,
                    config,
                    &declared_prefixes,
                )?
                .map(str::to_owned),
            });
    }
    Ok(map)
}

/// The prefix declared for the registry `pnpm publish` would publish the
/// project to: `publishConfig["@<scope>:registry"]`, then
/// `publishConfig.registry`, then the scope's registry, then the default
/// registry, read from the [`publication_manifest`]. The scope is that of the
/// published name, which `publishConfig.name` may rename.
fn publish_registry_prefix<'prefixes>(
    project: &Project,
    name: &str,
    config: &Config,
    declared_prefixes: &'prefixes DeclaredRegistryPrefixes,
) -> Result<Option<&'prefixes str>, PackageManifestError> {
    if declared_prefixes.is_empty() {
        return Ok(None);
    }
    let manifest = publication_manifest(project)?;
    let published_name = manifest
        .get("publishConfig")
        .and_then(|publish_config| publish_config.get("name"))
        .or_else(|| manifest.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(name);
    let Ok(registry) = find_registry_info(
        published_name,
        &config.registry,
        &config.registries_by_scope,
        publish_config_registry(&manifest, published_name),
    ) else {
        return Ok(None);
    };
    Ok(declared_prefixes.prefix_for(registry.as_str()))
}

/// The manifest `pnpm publish` selects the registry from. A project that
/// publishes from `publishConfig.directory` packs that directory's manifest,
/// so it is read from there once it exists; until then, and for every other
/// project, the project's own manifest is what will be published.
fn publication_manifest(project: &Project) -> Result<Cow<'_, Value>, PackageManifestError> {
    let manifest = project.manifest.value();
    let Some(directory) = manifest
        .get("publishConfig")
        .and_then(|publish_config| publish_config.get("directory"))
        .and_then(Value::as_str)
        .filter(|directory| !directory.is_empty())
    else {
        return Ok(Cow::Borrowed(manifest));
    };
    Ok(safe_read_project_manifest_from_dir(&project.root_dir.join(directory))?
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed(manifest)))
}

/// The workspace's packages, or `None` outside a workspace or when its
/// projects cannot be discovered. A sibling's unreadable publication manifest
/// is an error: the prefix of a dependency on it would be guessed.
pub fn discover_workspace_package_manifests(
    workspace_dir: Option<&Path>,
    config: &Config,
) -> Result<Option<Arc<WorkspacePackageManifests>>, PackageManifestError> {
    let Some(ws_dir) = workspace_dir else {
        return Ok(None);
    };
    let Ok((projects, _)) = crate::cli_args::recursive::discover_workspace_projects(ws_dir, config)
    else {
        return Ok(None);
    };
    build_workspace_package_manifest_map(&projects, config).map(Arc::new).map(Some)
}

pub fn create_publish_pack_manifest_options(
    manifest_flags: &PublishManifestArgs,
    config: &Config,
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    workspace_packages: Option<Arc<WorkspacePackageManifests>>,
) -> miette::Result<PackManifestOptions> {
    Ok(PackManifestOptions {
        catalogs: crate::cli_args::catalogs::configured_catalogs(config)?,
        catalogs_dir: config.workspace_dir.clone(),
        embed_readme: resolve_bool_override(
            manifest_flags.embed_readme,
            manifest_flags.no_embed_readme,
            config.embed_readme,
        ),
        node_linker: config.node_linker,
        skip_obfuscation: resolve_bool_override(
            manifest_flags.skip_manifest_obfuscation,
            manifest_flags.no_skip_manifest_obfuscation,
            config.skip_manifest_obfuscation,
        ),
        before_packing_hooks: before_packing_hooks.to_vec(),
        workspace_packages,
    })
}

#[cfg(test)]
mod tests;
