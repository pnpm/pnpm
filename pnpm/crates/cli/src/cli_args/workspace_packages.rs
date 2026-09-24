use crate::cli_args::{install::resolve_bool_override, publish::PublishManifestArgs};
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{PackManifestOptions, WorkspacePackageManifest};
use std::{collections::HashMap, path::Path, sync::Arc};

pub fn build_workspace_package_manifest_map(
    projects: &[pnpm_workspace::Project],
) -> HashMap<String, WorkspacePackageManifest> {
    let mut map = HashMap::new();
    for project in projects {
        let manifest = project.manifest.value();
        if let Some(name) = manifest.get("name").and_then(|val| val.as_str()) {
            let version = manifest
                .get("version")
                .and_then(|val| val.as_str())
                .unwrap_or("");
            let entry = map
                .entry(name.to_string())
                .or_insert_with(|| WorkspacePackageManifest {
                    name: name.to_string(),
                    version: version.to_string(),
                });
            if entry.version.is_empty() && !version.is_empty() {
                entry.version = version.to_string();
            }
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
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    workspace_packages: Option<Arc<HashMap<String, WorkspacePackageManifest>>>,
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
mod tests {
    use std::path::PathBuf;

    use pnpm_package_manifest::PackageManifest;
    use pnpm_workspace::Project;
    use serde_json::json;

    use super::build_workspace_package_manifest_map;

    fn project(root_dir: &str, name: &str, version: Option<&str>) -> Project {
        let root_dir = PathBuf::from(root_dir);
        let mut manifest = json!({ "name": name });
        if let Some(version) = version {
            manifest["version"] = json!(version);
        }
        Project {
            root_dir: root_dir.clone(),
            manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
            dependency_manifest: None,
        }
    }

    #[test]
    fn versioned_manifest_replaces_an_earlier_name_only_manifest() {
        let map = build_workspace_package_manifest_map(&[
            project("/workspace/incomplete", "pkg-b", None),
            project("/workspace/complete", "pkg-b", Some("2.0.0")),
        ]);

        assert_eq!(map["pkg-b"].version, "2.0.0");
    }

    #[test]
    fn first_complete_manifest_wins_over_a_later_complete_manifest() {
        let map = build_workspace_package_manifest_map(&[
            project("/workspace/first", "pkg-b", Some("1.0.0")),
            project("/workspace/second", "pkg-b", Some("2.0.0")),
        ]);

        assert_eq!(map["pkg-b"].version, "1.0.0");
    }
}
