use super::{ResolvedOverride, VersionsOverrider, remove_peer_dependency};
use serde_json::Value;

impl VersionsOverrider {
    pub(super) fn remove_meta_only_peers(
        &self,
        value: &mut Value,
        applicable_parent_scoped: &[&ResolvedOverride],
    ) {
        let removed: Vec<String> = meta_only_peer_names(value)
            .filter(|name| self.is_removed_peer(applicable_parent_scoped, name))
            .map(str::to_string)
            .collect();
        for name in removed {
            remove_peer_dependency(value, &name);
        }
    }

    pub(super) fn has_meta_only_peer_removal(
        &self,
        value: &Value,
        applicable_parent_scoped: &[&ResolvedOverride],
    ) -> bool {
        meta_only_peer_names(value).any(|name| self.is_removed_peer(applicable_parent_scoped, name))
    }

    fn is_removed_peer(&self, applicable_parent_scoped: &[&ResolvedOverride], name: &str) -> bool {
        self.choose_override(applicable_parent_scoped, name, "*")
            .is_some_and(|chosen| chosen.inner.new_bare_specifier == "-")
    }
}

/// The `peerDependenciesMeta` names that `peerDependencies` does not
/// declare. The resolver treats such an entry as an optional `"*"` peer.
fn meta_only_peer_names(value: &Value) -> impl Iterator<Item = &str> {
    let peers = value.get("peerDependencies").and_then(Value::as_object);
    value
        .get("peerDependenciesMeta")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|meta| meta.keys())
        .filter(move |name| !peers.is_some_and(|declared| declared.contains_key(*name)))
        .map(String::as_str)
}
