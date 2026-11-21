//! A `nodeLinker` object option and its flat alias name one setting, so a
//! command-line choice made in either spelling is protected from an
//! `updateConfig` hook that uses the other.

use pnpm_config::{
    Config,
    NodeLinkerSetting,
};
use serde_json::{
    Map,
    Value,
};

const HOIST_LISTS: [&str; 2] = ["private", "public"];

/// Drop the hook's changes to settings the command line set in the other
/// spelling.
pub(super) fn drop_cli_linker_aliases(config: &Config, delta: &mut Map<String, Value>) {
    if let Some(Value::Object(linker)) = delta.get_mut("nodeLinker") {
        drop_cli_set_options(config, linker);
    }
    if !config.cli_settings.contains("nodeLinker") {
        return;
    }
    let Some(cli_linker) = config.explicit_settings.get("nodeLinker") else { return };
    for option in options_set(cli_linker) {
        for key in flat_aliases(option) {
            delta.remove(*key);
        }
    }
}

/// Reapply the hook's `nodeLinker` object, whose options win over a flat
/// alias the same hook set to null and that was reset to its default.
pub(super) fn reapply_linker_options(config: &mut Config, delta: &Value) {
    let Some(linker) = delta.get("nodeLinker").filter(|linker| linker.is_object()) else {
        return;
    };
    if let Ok(setting) = serde_json::from_value::<NodeLinkerSetting>(linker.clone()) {
        setting.apply_to(config);
    }
}

fn drop_cli_set_options(config: &Config, linker: &mut Map<String, Value>) {
    let set_on_cli = |option: &str| {
        flat_aliases(option)
            .iter()
            .any(|key| config.cli_settings.contains(*key))
    };
    match linker.get_mut("hoist") {
        // `hoist: true` and `hoist: false` set both lists at once.
        Some(Value::Bool(_)) if HOIST_LISTS.into_iter().any(set_on_cli) => {
            linker.remove("hoist");
        }
        Some(Value::Object(hoist)) => hoist.retain(|option, _| !set_on_cli(option)),
        _ => {}
    }
    if set_on_cli("hoistingLimits") {
        linker.remove("hoistingLimits");
    }
}

/// The options a `nodeLinker` value sets.
fn options_set(linker: &Value) -> Vec<&'static str> {
    let mut options = Vec::new();
    match linker.get("hoist") {
        Some(Value::Bool(_)) => options.extend(HOIST_LISTS),
        Some(Value::Object(hoist)) => options.extend(
            HOIST_LISTS
                .into_iter()
                .filter(|list| hoist.get(*list).is_some_and(is_set)),
        ),
        _ => {}
    }
    if linker.get("hoistingLimits").is_some_and(is_set) {
        options.push("hoistingLimits");
    }
    options
}

fn is_set(value: &Value) -> bool {
    !value.is_null()
}

/// The flat settings that a `nodeLinker` option aliases.
fn flat_aliases(option: &str) -> &'static [&'static str] {
    match option {
        "private" => &["hoist", "hoistPattern"],
        "public" => &["publicHoistPattern", "shamefullyHoist"],
        "hoistingLimits" => &["hoistingLimits"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests;
