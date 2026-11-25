use pnpm_config::Config;
use serde_json::{
    Map,
    Value,
};

pub(super) fn drop_cli_map_policies(config: &Config, delta: &mut Map<String, Value>) {
    for (name, marker, option, aliases) in [
        ("catalog", "entries", "mode", &["catalogMode"][..]),
        ("catalog", "entries", "defaultSaveTarget", &["saveCatalogName"][..]),
        ("catalog", "entries", "unused", &["catalogPrune", "cleanupUnusedCatalogs"][..]),
        ("overrides", "rules", "unused", &["warnUnusedOverrides"][..]),
    ] {
        let cli_nested = config.cli_settings.contains(name)
            && config.explicit_settings
                .get(name)
                .is_some_and(|value| has_option(value, marker, option));
        let protected = cli_nested
            || aliases
                .iter()
                .any(|alias| config.cli_settings.contains(*alias));
        if !protected && !delta.get(name).is_some_and(|value| has_option(value, marker, option)) {
            continue;
        }
        for alias in aliases {
            delta.remove(*alias);
        }
        if protected {
            drop_nested_option(delta.get_mut(name), marker, option);
        }
    }
}

fn drop_nested_option(section: Option<&mut Value>, marker: &str, option: &str) {
    if let Some(section) =
        section.filter(|value| has_option(value, marker, option)).and_then(Value::as_object_mut)
    {
        section.remove(option);
    }
}

fn has_option(value: &Value, marker: &str, option: &str) -> bool {
    value.get(marker).is_some_and(Value::is_object)
        && value.get(option).is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests;
