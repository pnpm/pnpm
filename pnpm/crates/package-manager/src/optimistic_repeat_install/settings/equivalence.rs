//! Recorded and current values that differ in form but mean the same setting.

use pnpm_workspace_state::WorkspaceStateSettings;
use std::path::Path;

/// `enableGlobalVirtualStore` has no `?? default` coercion on pnpm's
/// read side, but its `undefined` default and an explicit `false` both
/// mean "global virtual store off". pnpm omits the key for the former
/// and records `false` only when CI forces it; pacquet omits both.
/// Normalize the absent and `false` forms before comparing so a
/// pnpm-written file (omitted or `false`) matches a pacquet install
/// with the store off, while a real `true`/`false` flip still trips.
pub(super) fn enable_global_virtual_store_match(
    state_value: Option<bool>,
    current_value: Option<bool>,
) -> bool {
    state_value.unwrap_or(false) == current_value.unwrap_or(false)
}

/// Pnpm writes `Some({})` for an empty `allowBuilds`; pacquet writes
/// `None` for the same effective value. Treat them as equivalent so
/// cross-package-manager state files don't trip the comparison.
pub(super) fn allow_builds_match(
    state_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
    current_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> bool {
    match (state_value, current_value) {
        (None, None) => true,
        (Some(map), None) | (None, Some(map)) => map.is_empty(),
        (Some(state_map), Some(current_map)) => state_map == current_map,
    }
}

/// `packageExtensions` are compared as opaque `serde_json::Value`
/// trees so the workspace-state file written by either implementation
/// round-trips through the other. Empty maps are equivalent to absent
/// — pacquet's [`pnpm_config::WorkspaceSettings::apply_to`] already collapses
/// `packageExtensions: {}` to `None`, but pnpm may write `Some({})`
/// directly, and the workspace-state file is shared across the two.
pub(super) fn package_extensions_match(
    state_value: Option<&serde_json::Value>,
    current_value: Option<&serde_json::Value>,
) -> bool {
    fn is_empty(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.is_empty(),
            serde_json::Value::Null => true,
            _ => false,
        }
    }
    match (state_value, current_value) {
        (None, None) => true,
        (Some(value), None) | (None, Some(value)) => is_empty(value),
        (Some(state_value), Some(current_value)) => state_value == current_value,
    }
}

/// pnpm v11 records each patch file resolved against the workspace
/// directory; pacquet records it as configured. A relative path is read from
/// `workspace_dir`, so resolving both forms there compares the files they
/// name.
pub(super) fn patched_dependencies_match(
    recorded: &WorkspaceStateSettings,
    live: &WorkspaceStateSettings,
    workspace_dir: Option<&Path>,
) -> bool {
    match (&recorded.patched_dependencies, &live.patched_dependencies, workspace_dir) {
        (Some(recorded), Some(live), Some(workspace_dir)) => {
            recorded.len() == live.len()
                && recorded
                    .iter()
                    .all(|(selector, recorded_path)| {
                        live.get(selector)
                            .is_some_and(|live_path| {
                                workspace_dir.join(recorded_path) == workspace_dir.join(live_path)
                            })
                    })
        }
        (recorded, live, _) => recorded == live,
    }
}
