use super::{
    Config, ConfigError, ConfigFlags, DEFAULT_JSR_REGISTRY, GLOBAL_CONFIG_YAML_FILENAME, IndexMap,
    Map, Path, Segment, Value, config_types, naming_cases, property_path, protected_settings,
};

/// `castField`: coerce a string value per its key's type. Booleans, `null`,
/// and `undefined` literals are recognized; numeric-typed keys parse to a
/// number; everything else is the trimmed string. Non-string values pass
/// through unchanged. `undefined` maps to JSON `null` (a deletion downstream).
pub(super) fn cast_field(value: Value, kebab_key: &str) -> Value {
    let Value::String(raw) = value else {
        return value;
    };
    let trimmed = raw.trim();
    match trimmed {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "undefined" => return Value::Null,
        _ => {}
    }
    if config_types::type_includes_number(kebab_key)
        && let Some(number) = parse_number(trimmed)
    {
        return Value::Number(number);
    }
    Value::String(trimmed.to_string())
}

/// Parse a trimmed numeric string into a JSON number (integer when whole),
/// matching JS `Number(value)` for the integer/decimal config values pnpm
/// accepts.
fn parse_number(value: &str) -> Option<serde_json::Number> {
    if let Ok(int) = value.parse::<i64>() {
        return Some(int.into());
    }
    value.parse::<f64>().ok().and_then(serde_json::Number::from_f64)
}

/// `validateSimpleKey`: a strictly-kebab-case key passes through; otherwise the
/// key must parse to a single property-path segment.
pub(super) fn validate_simple_key(key: &str) -> Result<String, ConfigError> {
    if naming_cases::is_strictly_kebab_case(key) {
        return Ok(key.to_string());
    }
    let segments =
        property_path::parse_property_path(key).map_err(ConfigError::InvalidPropertyPath)?;
    match segments.as_slice() {
        [] => Err(ConfigError::SetEmptyKey),
        [single] => Ok(segment_to_string(single)),
        _ => Err(ConfigError::SetDeepKey),
    }
}

fn segment_to_string(segment: &Segment) -> String {
    match segment {
        Segment::Key(key) => key.clone(),
        Segment::Index(n) => format!("{}", *n as i64),
    }
}

/// `validateWorkspaceKey`: a known `types` key becomes camelCase; otherwise it
/// must already be camelCase.
pub(super) fn validate_workspace_key(key: &str) -> Result<String, ConfigError> {
    if config_types::is_type_key(key) || config_types::is_config_file_key(key) {
        return Ok(naming_cases::to_camel_case(key));
    }
    if !naming_cases::is_camel_case(key) {
        return Err(ConfigError::SetUnsupportedWorkspaceKey {
            key: key.to_string(),
            camel: naming_cases::to_camel_case(key),
        });
    }
    Ok(key.to_string())
}

/// `validateIniConfigKey`: the kebab-case key must be a known `types` key.
pub(super) fn validate_ini_config_key(key: &str) -> Result<String, ConfigError> {
    let kebab = naming_cases::to_kebab_case(key);
    if config_types::is_type_key(&kebab) {
        return Ok(kebab);
    }
    Err(ConfigError::SetUnsupportedIniConfigKey {
        key: key.to_string(),
        camel: naming_cases::to_camel_case(key),
    })
}

/// `validateYamlConfigKey`: the kebab-case key must be valid in the global
/// `config.yaml`.
pub(super) fn validate_yaml_config_key(key: &str) -> Result<String, ConfigError> {
    let kebab = naming_cases::to_kebab_case(key);
    if config_types::is_config_file_key(&kebab) {
        return Ok(kebab);
    }
    Err(ConfigError::SetUnsupportedYamlConfigKey { key: key.to_string() })
}

const STRING_ONLY_INI_KEYS: &[&str] = &["_auth", "_authToken", "_password", "username", "registry"];

pub(super) fn is_string_only_ini_key(key: &str) -> bool {
    STRING_ONLY_INI_KEYS.contains(&key) || key.starts_with('@') || key.starts_with("//")
}

// ---------------------------------------------------------------------------
// config get / list
// ---------------------------------------------------------------------------

/// `configGet`: resolve and render the value at `key`. Port of `configGet`.
pub(super) fn config_get(
    config: &Config,
    flags: ConfigFlags,
    key: &str,
) -> Result<String, ConfigError> {
    let is_scoped = key.starts_with('@');
    let value = match lookup_config(config, key, is_scoped) {
        Some(value) => value,
        None if is_property_path(key) => lookup_by_property_path(config, key)?,
        None => Value::Null,
    };
    Ok(display_config(&value, flags.json))
}

/// pnpm resolves relative patch paths against the workspace root when it
/// reads the setting, so its record shows the resolved paths; pacquet keeps
/// them as written and resolves lazily — mirror the resolved display.
fn absolutize_patch_paths(result: &mut Map<String, Value>, config: &Config) {
    let Some(workspace_dir) = &config.workspace_dir else { return };
    let Some(Value::Object(patched)) = result.get_mut("patchedDependencies") else { return };
    for value in patched.values_mut() {
        if let Value::String(path) = value
            && !Path::new(path.as_str()).is_absolute()
        {
            *value =
                Value::String(workspace_dir.join(path.as_str()).to_string_lossy().into_owned());
        }
    }
}

/// `catalogs` shows the complete resolved catalog set — the singular
/// `catalog` block is its `default` entry, listed first — whichever spelling
/// declared it.
fn merge_default_catalog(result: &mut Map<String, Value>) {
    let named = match result.remove("catalogs") {
        Some(Value::Object(named)) => named,
        _ => Map::new(),
    };
    let default = result.get("catalog").or_else(|| named.get("default")).cloned();
    let mut merged = Map::new();
    if let Some(default) = default {
        merged.insert("default".to_string(), default);
    }
    for (name, catalog) in named {
        if name != "default" {
            merged.insert(name, catalog);
        }
    }
    if !merged.is_empty() {
        result.insert("catalogs".to_string(), Value::Object(merged));
    }
}

/// `configList`: the full config record as pretty JSON. Port of `configList`.
pub(super) fn config_list(config: &Config) -> String {
    serde_json::to_string_pretty(&Value::Object(config_to_record(config)))
        .expect("serializing the config record to JSON never fails")
}

/// `lookupConfig`: resolve `key` against scoped registries, `globalconfig`,
/// the typed settings, the raw auth keys, or the config record. `None` means
/// "not found, fall through to a property-path lookup".
fn lookup_config(config: &Config, key: &str, is_scoped: bool) -> Option<Value> {
    if is_scoped {
        return Some(lookup_scoped_config(config, key));
    }
    if key == "globalconfig" {
        let path = config
            .config_dir
            .as_ref()
            .map(|dir| dir.join(GLOBAL_CONFIG_YAML_FILENAME).to_string_lossy().into_owned())
            .unwrap_or_default();
        return Some(Value::String(path));
    }
    let kebab = if naming_cases::is_camel_case(key) {
        naming_cases::to_kebab_case(key)
    } else {
        key.to_string()
    };
    // The merged default is what resolvers use, so `registry` answers the
    // same URL the resolved `registries` view declares as the bare `@`
    // scope — a raw `.npmrc` value would contradict it.
    if kebab == "registry" {
        return Some(Value::String(config.registry.clone()));
    }
    if config_types::is_type_key(&kebab) {
        return Some(lookup_typed_config(config, &kebab));
    }
    if config_types::is_ini_config_key(key) {
        return Some(auth_value(config, key));
    }
    // Not in `types` (e.g. packageExtensions): look it up in the record, which
    // excludes internal/sensitive fields.
    let camel = naming_cases::to_camel_case(key);
    let record = config_to_record(config);
    record.get(&camel).cloned()
}

fn lookup_scoped_config(config: &Config, key: &str) -> Value {
    let Some(scope) = key.strip_suffix(":registry") else {
        return auth_value(config, key);
    };
    // Prefer the merged `registries` map so this reports the same URL
    // resolvers/publish use (pnpm/pnpm#11492).
    if let Some(merged) = config.registries_by_scope.get(scope) {
        return Value::String(merged.clone());
    }
    // The built-in `@jsr` route, which pnpm merges into its scope map.
    if scope == "@jsr" && !config.raw_auth_config.contains_key(key) {
        return Value::String(DEFAULT_JSR_REGISTRY.to_string());
    }
    auth_value(config, key)
}

/// A key the config `types` declare: the explicitly set value, the raw
/// auth-file value, or null.
fn lookup_typed_config(config: &Config, kebab: &str) -> Value {
    let camel = naming_cases::to_camel_case(kebab);
    if let Some(value) = config.explicit_settings.get(&camel) {
        return value.clone();
    }
    config.raw_auth_config.get(kebab).map_or(Value::Null, |value| Value::String(value.clone()))
}

fn auth_value(config: &Config, key: &str) -> Value {
    config.raw_auth_config.get(key).map_or(Value::Null, |value| Value::String(value.clone()))
}

/// `lookupByPropertyPath`: resolve a (possibly nested) property path against the
/// config record. An empty path returns the whole record.
fn lookup_by_property_path(config: &Config, property_path: &str) -> Result<Value, ConfigError> {
    let segments = parse_config_property_path(property_path)?;
    let record = Value::Object(config_to_record(config));
    if segments.is_empty() {
        return Ok(record);
    }
    Ok(property_path::get_object_value_by_property_path(&record, &segments)
        .cloned()
        .unwrap_or(Value::Null))
}

/// `parseConfigPropertyPath`: like `parsePropertyPath` but with the first
/// string segment camelCased to match the record's camelCase keys.
fn parse_config_property_path(property_path: &str) -> Result<Vec<Segment>, ConfigError> {
    let mut segments = property_path::parse_property_path(property_path)
        .map_err(ConfigError::InvalidPropertyPath)?;
    if let Some(Segment::Key(first)) = segments.first_mut() {
        *first = naming_cases::to_camel_case(first);
    }
    Ok(segments)
}

fn is_property_path(key: &str) -> bool {
    key.is_empty() || key.contains('.') || key.contains('[')
}

/// `displayConfig`: JSON for objects/arrays (and always under `--json`), the
/// plain string form otherwise.
fn display_config(value: &Value, json: bool) -> String {
    if json || value.is_array() || value.is_object() {
        serde_json::to_string_pretty(value).expect("serializing a config value to JSON never fails")
    } else {
        plain_string(value)
    }
}

/// The `String(value)` rendering pnpm uses for a non-object scalar.
fn plain_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // pacquet represents both "undefined" and JSON null as `Null`; the
        // command only ever produces it for an unset key, which pnpm renders
        // as `String(undefined)`.
        Value::Null => "undefined".to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// `configToRecord`: build the camelCase record shown by `list` / `get` — the
/// explicitly-set settings, then the raw auth keys (original casing), then
/// `userAgent` — sorted by key and with protected settings censored.
/// `registries` is always present and holds the resolved view: the registries
/// the CLI resolves from, merged across every source, in place of the raw
/// `registries` / `namedRegistries` values a single source set.
fn config_to_record(config: &Config) -> Map<String, Value> {
    let mut result: Map<String, Value> = Map::new();
    for (key, value) in &config.explicit_settings {
        result.insert(key.clone(), value.clone());
    }
    result.remove("namedRegistries");
    result.insert(
        "registries".to_string(),
        serde_json::to_value(config.resolved_registry_declarations())
            .expect("serializing registry declarations to JSON never fails"),
    );
    record_command_settings(&mut result, config);
    merge_default_catalog(&mut result);
    absolutize_patch_paths(&mut result, config);
    for (key, value) in &config.raw_auth_config {
        result.entry(key.clone()).or_insert_with(|| Value::String(value.clone()));
    }
    // The `registry` / `@scope:registry` rows show the merged routes — the
    // values `config get` answers — so a raw `.npmrc` row cannot contradict
    // the resolved `registries` view.
    result.insert("registry".to_string(), Value::String(config.registry.clone()));
    for (scope, url) in &config.registries_by_scope {
        if scope != "default" {
            result.insert(format!("{scope}:registry"), Value::String(url.clone()));
        }
    }
    result
        .entry("@jsr:registry".to_string())
        .or_insert_with(|| Value::String(DEFAULT_JSR_REGISTRY.to_string()));
    if !config.user_agent.is_empty() {
        result.insert("userAgent".to_string(), Value::String(config.user_agent.clone()));
    }

    // sortDirectKeys: order the top-level keys lexicographically.
    let mut sorted: IndexMap<String, Value> = result.into_iter().collect();
    sorted.sort_keys();
    let mut censored: Map<String, Value> = sorted.into_iter().collect();
    protected_settings::censor_protected_settings(&mut censored);
    censored
}

fn record_command_settings(result: &mut Map<String, Value>, config: &Config) {
    for key in ["update", "updateConfig", "audit", "auditConfig", "auditLevel"] {
        result.remove(key);
    }
    if let Some(update) = config.resolved_update_settings() {
        result.insert(
            "update".to_string(),
            serde_json::to_value(update).expect("serializing update settings to JSON never fails"),
        );
    }
    if let Some(audit) = config.resolved_audit_settings() {
        result.insert(
            "audit".to_string(),
            serde_json::to_value(audit).expect("serializing audit settings to JSON never fails"),
        );
    }
}
