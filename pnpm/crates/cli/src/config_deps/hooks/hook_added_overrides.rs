//! Records the overrides an `updateConfig` hook adds in
//! [`Config::hook_added_overrides`].

use pnpm_config::Config;
use serde_json::Value;
use std::collections::HashSet;

/// Selectors are compared and recorded trimmed, the form the overrides
/// parser gives them.
pub(super) fn record_hook_added_overrides(config: &mut Config, input: &Value, delta: &Value) {
    let Some(overrides) = delta
        .get("overrides")
        .and_then(|value| pnpm_workspace::map_settings::package_map(value, "rules"))
    else {
        return;
    };
    let declared: HashSet<&str> = input
        .get("overrides")
        .and_then(|value| pnpm_workspace::map_settings::package_map(value, "rules"))
        .into_iter()
        .flat_map(serde_json::Map::keys)
        .map(|selector| selector.trim())
        .collect();
    let added = overrides
        .keys()
        .map(|selector| selector.trim())
        .filter(|selector| !declared.contains(selector));
    config.hook_added_overrides.extend(added.map(str::to_string));
}
