use super::Config;
use serde_json::{Map, Number, Value};

pub(super) fn deploy_workspace_manifest(config: &Config) -> Map<String, Value> {
    Map::from_iter([
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
    ])
}
