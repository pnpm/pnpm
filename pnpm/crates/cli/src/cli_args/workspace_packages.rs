use crate::cli_args::{
    install::resolve_bool_override,
    publish::PublishManifestArgs,
};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    PackManifestOptions,
    WorkspacePackageManifest,
};
use std::{
    collections::{
        HashMap,
        HashSet,
        hash_map::Entry,
    },
    path::Path,
    sync::Arc,
};

pub fn build_workspace_package_manifest_map(
    projects: &[pnpm_workspace::Project],
) -> HashMap<String, WorkspacePackageManifest> {
    let mut map: HashMap<String, WorkspacePackageManifest> = HashMap::new();
    for project in projects {
        let manifest = WorkspacePackageManifest::from_manifest(project.manifest.value());
        if manifest.name.is_empty() {
            continue;
        }
        match map.entry(manifest.name.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(manifest);
            }
            Entry::Occupied(mut entry)
                if entry.get().version.is_empty() && !manifest.version.is_empty() =>
            {
                let mut manifest = manifest;
                manifest.private |= entry.get().private;
                entry.insert(manifest);
            }
            Entry::Occupied(_) => {}
        }
    }
    map
}

pub fn discover_workspace_package_manifests(
    workspace_dir: Option<&Path>,
    config: &Config,
) -> Option<Arc<HashMap<String, WorkspacePackageManifest>>> {
    let ws_dir = workspace_dir?;
    let (projects, _) =
        crate::cli_args::recursive::discover_workspace_projects(ws_dir, config).ok()?;
    Some(Arc::new(build_workspace_package_manifest_map(&projects)))
}

pub fn create_publish_pack_manifest_options(
    manifest_flags: &PublishManifestArgs,
    config: &Config,
    before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    workspace_packages: Option<Arc<HashMap<String, WorkspacePackageManifest>>>,
    bumped_packages: Option<&Arc<HashSet<String>>>,
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
        before_packing_hooks,
        workspace_packages,
        bumped_workspace_packages: bumped_packages.map(Arc::clone),
    })
}

#[cfg(test)]
mod tests;
