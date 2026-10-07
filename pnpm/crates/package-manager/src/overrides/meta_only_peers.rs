use super::{ResolvedOverride, VersionsOverrider, remove_peer_dependency};
use serde_json::Value;

impl VersionsOverrider {
    pub(super) fn remove_meta_only_peers(
        &self,
        value: &mut Value,
        applicable_parent_scoped: &[&ResolvedOverride],
    ) {
        for name in meta_only_peer_names(value) {
            if self.removes_meta_only_peer(applicable_parent_scoped, &name) {
                remove_peer_dependency(value, &name);
            }
        }
    }

    pub(super) fn removes_a_meta_only_peer(
        &self,
        value: &Value,
        applicable_parent_scoped: &[&ResolvedOverride],
    ) -> bool {
        meta_only_peer_names(value)
            .iter()
            .any(|name| self.removes_meta_only_peer(applicable_parent_scoped, name))
    }

    fn removes_meta_only_peer(
        &self,
        applicable_parent_scoped: &[&ResolvedOverride],
        name: &str,
    ) -> bool {
        self.choose_override(applicable_parent_scoped, name, "*")
            .is_some_and(|chosen| chosen.inner.new_bare_specifier == "-")
    }
}

/// The `peerDependenciesMeta` names that `peerDependencies` does not
/// declare. The resolver treats such an entry as an optional `"*"` peer.
fn meta_only_peer_names(value: &Value) -> Vec<String> {
    let peers = value.get("peerDependencies").and_then(Value::as_object);
    value
        .get("peerDependenciesMeta")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|meta| meta.keys())
        .filter(|name| !peers.is_some_and(|peers| peers.contains_key(*name)))
        .cloned()
        .collect()
}
