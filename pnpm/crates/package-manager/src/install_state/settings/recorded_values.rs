//! Config values in the form the workspace state records them.

use pnpm_catalogs_types::Catalogs;
use pnpm_config::{LinkWorkspacePackages, NodeLinker, TrustPolicy};
use pnpm_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, TrustPolicy as WorkspaceStateTrustPolicy,
};

pub(crate) fn catalogs_to_json(catalogs: &Catalogs) -> serde_json::Value {
    serde_json::to_value(catalogs).expect("Catalogs serialize to a JSON object")
}

pub(crate) fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

pub(crate) fn filter_null_object_values(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = value else { return value };
    map.retain(|_, value| !value.is_null());
    serde_json::Value::Object(map)
}

pub(crate) fn link_workspace_packages_to_json(value: LinkWorkspacePackages) -> serde_json::Value {
    match value {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    }
}

pub(crate) fn map_node_linker(linker: NodeLinker) -> WorkspaceStateNodeLinker {
    match linker {
        NodeLinker::Isolated => WorkspaceStateNodeLinker::Isolated,
        NodeLinker::Hoisted => WorkspaceStateNodeLinker::Hoisted,
        NodeLinker::Pnp => WorkspaceStateNodeLinker::Pnp,
        NodeLinker::Loaded => WorkspaceStateNodeLinker::Loaded,
    }
}

pub(super) fn map_trust_policy(policy: TrustPolicy) -> WorkspaceStateTrustPolicy {
    match policy {
        TrustPolicy::Off => WorkspaceStateTrustPolicy::Off,
        TrustPolicy::NoDowngrade => WorkspaceStateTrustPolicy::NoDowngrade,
    }
}
