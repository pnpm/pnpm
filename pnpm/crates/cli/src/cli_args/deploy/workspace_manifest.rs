use super::Config;
use serde_json::{Map, Number, Value};

pub(super) fn deploy_workspace_manifest(config: &Config) -> Map<String, Value> {
    let mut manifest = Map::from_iter([
        ("dedupeInjectedDeps".to_string(), Value::Bool(false)),
        ("dedupePeerDependents".to_string(), Value::Bool(false)),
        ("injectWorkspacePackages".to_string(), Value::Bool(false)),
        ("packages".to_string(), Value::Array(vec![Value::String(".".to_string())])),
        ("virtualStoreType".to_string(), Value::String("project".to_string())),
    ]);
    if !config.auto_install_peers {
        manifest.insert("autoInstallPeers".to_string(), Value::Bool(false));
    }
    if config.dedupe_peers {
        manifest.insert("dedupePeers".to_string(), Value::Bool(true));
    }
    if config.exclude_links_from_lockfile {
        manifest.insert("excludeLinksFromLockfile".to_string(), Value::Bool(true));
    }
    if let Some(ignored) = &config.ignored_optional_dependencies
        && !ignored.is_empty()
    {
        manifest.insert(
            "ignoredOptionalDependencies".to_string(),
            Value::Array(
                ignored
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if config.peers_suffix_max_length != pnpm_config::default_peers_suffix_max_length() {
        manifest.insert(
            "peersSuffixMaxLength".to_string(),
            Value::Number(Number::from(config.peers_suffix_max_length)),
        );
    }
    manifest
}

pub(super) fn workspace_manifest_yaml(workspace_manifest: &Value) -> String {
    let mut out = String::new();
    let Some(object) = workspace_manifest.as_object() else { return out };
    for field in [
        "autoInstallPeers",
        "dedupeInjectedDeps",
        "dedupePeerDependents",
        "dedupePeers",
        "excludeLinksFromLockfile",
        "ignoredOptionalDependencies",
        "injectWorkspacePackages",
        "packages",
        "peersSuffixMaxLength",
        "virtualStoreType",
    ] {
        let Some(value) = object.get(field) else { continue };
        out.push_str(field);
        out.push_str(": ");
        out.push_str(&value.to_string());
        out.push('\n');
    }
    for field in ["patchedDependencies", "allowBuilds"] {
        let Some(values) = object.get(field).and_then(Value::as_object) else { continue };
        out.push_str(field);
        out.push_str(":\n");
        for (key, value) in values {
            out.push_str("  ");
            out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}")));
            out.push_str(": ");
            out.push_str(&serde_json::to_string(value).unwrap_or_else(|_| value.to_string()));
            out.push('\n');
        }
    }
    out
}
