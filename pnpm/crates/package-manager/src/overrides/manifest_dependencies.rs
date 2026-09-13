use serde_json::Value;

/// Rewrite a peer's range, as long as the manifest still declares a
/// `peerDependencies` object.
pub(super) fn insert_peer_dependency(value: &mut Value, name: String, spec: String) {
    if let Some(peers) = value.get_mut("peerDependencies").and_then(Value::as_object_mut) {
        peers.insert(name, Value::String(spec));
    }
}

pub(super) fn remove_peer_dependency(value: &mut Value, name: &str) {
    if let Some(peers) = value.get_mut("peerDependencies").and_then(Value::as_object_mut) {
        peers.remove(name);
    }
}

/// An override value that is not a valid peer range moves the edge into
/// `dependencies`, creating that object when the manifest declares none.
pub(super) fn insert_regular_dependency(value: &mut Value, name: String, spec: String) {
    if !value.get("dependencies").is_some_and(Value::is_object)
        && let Some(root) = value.as_object_mut()
    {
        root.insert(
            "dependencies".to_string(),
            Value::Object(serde_json::Map::new()),
        );
    }
    if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
        deps.insert(name, Value::String(spec));
    }
}
