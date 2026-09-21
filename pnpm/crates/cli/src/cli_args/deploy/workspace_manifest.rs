use serde_json::{Map, Value};

pub(super) fn deploy_workspace_manifest() -> Map<String, Value> {
    Map::from_iter([
        ("dedupeInjectedDeps".to_string(), Value::Bool(false)),
        ("dedupePeerDependents".to_string(), Value::Bool(false)),
        ("injectWorkspacePackages".to_string(), Value::Bool(false)),
        ("packages".to_string(), Value::Array(vec![Value::String(".".to_string())])),
        ("virtualStoreType".to_string(), Value::String("project".to_string())),
    ])
}

pub(super) fn workspace_manifest_yaml(workspace_manifest: &Value) -> String {
    let mut out = String::new();
    let Some(object) = workspace_manifest.as_object() else { return out };
    for field in [
        "dedupeInjectedDeps",
        "dedupePeerDependents",
        "injectWorkspacePackages",
        "packages",
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
