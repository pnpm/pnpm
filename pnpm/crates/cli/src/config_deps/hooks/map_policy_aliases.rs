use pnpm_config::Config;
use serde_json::{
    Map,
    Value,
};

pub(super) fn drop_cli_map_policies(config: &Config, delta: &mut Map<String, Value>) {
    for (name, marker, aliases) in [
        ("catalog", "entries", &["catalogPrune", "cleanupUnusedCatalogs"][..]),
        ("overrides", "rules", &["warnUnusedOverrides"][..]),
    ] {
        let cli_nested = config.cli_settings.contains(name)
            && config.explicit_settings.get(name).is_some_and(|value| has_policy(value, marker));
        if !cli_nested
            && !aliases
                .iter()
                .any(|alias| config.cli_settings.contains(*alias))
        {
            continue;
        }
        for alias in aliases {
            delta.remove(*alias);
        }
        if let Some(section) = delta.get_mut(name).filter(|value| has_policy(value, marker))
            && let Some(section) = section.as_object_mut()
        {
            section.remove("unused");
        }
    }
}

fn has_policy(value: &Value, marker: &str) -> bool {
    value.get(marker).is_some_and(Value::is_object)
        && value.get("unused").is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests;
