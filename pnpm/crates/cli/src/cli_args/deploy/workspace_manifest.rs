use super::{
    Config, DeployWorkspaceConfig, HashMap, IntoDiagnostic, Lockfile, Path, PathBuf,
    install::configured_virtual_store_dir, relative_path,
};
use serde_json::{Map, Number, Value};

pub(super) fn deploy_workspace_manifest(config: &Config) -> Map<String, Value> {
    let mut manifest = Map::from_iter([
        ("autoInstallPeers".to_string(), Value::Bool(config.auto_install_peers)),
        ("dedupeInjectedDeps".to_string(), Value::Bool(false)),
        ("dedupePeerDependents".to_string(), Value::Bool(false)),
        ("dedupePeers".to_string(), Value::Bool(config.dedupe_peers)),
        ("excludeLinksFromLockfile".to_string(), Value::Bool(config.exclude_links_from_lockfile)),
        (
            "ignoredOptionalDependencies".to_string(),
            Value::Array(
                config.ignored_optional_dependencies
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        ),
        ("injectWorkspacePackages".to_string(), Value::Bool(false)),
        ("packages".to_string(), Value::Array(vec![Value::String(".".to_string())])),
        (
            "peersSuffixMaxLength".to_string(),
            Value::Number(Number::from(config.peers_suffix_max_length)),
        ),
        ("virtualStoreType".to_string(), Value::String("project".to_string())),
    ]);
    if cfg!(unix) && config.preserve_bin_name {
        manifest.insert("preserveBinName".to_string(), Value::Bool(true));
    }
    if let Some(virtual_store_dir) = configured_virtual_store_dir(config) {
        manifest.insert(
            "virtualStoreDir".to_string(),
            Value::String(virtual_store_dir.to_string()),
        );
    }
    manifest
}

/// The `pnpm-workspace.yaml` the deploy writes, and the same settings in
/// the shape the deploy install consumes. The manifest records the
/// self-contained deploy layout plus settings that survive from the source.
pub(super) fn deploy_workspace_settings(
    lockfile: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    deploy_lockfile: &mut Lockfile,
) -> miette::Result<(Map<String, Value>, DeployWorkspaceConfig)> {
    let mut workspace_manifest = deploy_workspace_manifest(config);
    let mut workspace_config =
        DeployWorkspaceConfig { patched_dependencies: None, allow_builds: HashMap::new() };
    if lockfile.patched_dependencies.is_some()
        && let Some(patched_dependencies) = config.patched_dependencies.as_ref()
    {
        deploy_lockfile.patched_dependencies.clone_from(&lockfile.patched_dependencies);
        let rewritten = patched_dependencies
            .iter()
            .map(|(name, value)| {
                let absolute = if Path::new(value).is_absolute() {
                    PathBuf::from(value)
                } else {
                    lockfile_dir.join(value)
                };
                (name.clone(), relative_path(deploy_dir, &absolute))
            })
            .collect::<indexmap::IndexMap<_, _>>();
        workspace_manifest.insert(
            "patchedDependencies".to_string(),
            serde_json::to_value(&rewritten).into_diagnostic()?,
        );
        workspace_config.patched_dependencies = Some(rewritten);
    }
    if !config.allow_builds.is_empty() {
        workspace_manifest.insert(
            "allowBuilds".to_string(),
            serde_json::to_value(&config.allow_builds).into_diagnostic()?,
        );
        workspace_config.allow_builds.clone_from(&config.allow_builds);
    }
    Ok((workspace_manifest, workspace_config))
}
