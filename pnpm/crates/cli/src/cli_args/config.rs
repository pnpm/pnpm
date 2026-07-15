//! `pacquet config` — manage the pnpm configuration files.
//!
//! Provides the `set` / `get` / `delete` / `list` subcommands, their file
//! routing, key validation and value casting, and the read path.
//!
//! pacquet's [`Config`] is the loaded, merged config rather than pnpm's
//! injected `_config` / `_context`: `pnpm config list` / `get` read the
//! explicitly-set settings from [`Config::explicit_settings`] and the raw auth
//! keys from [`Config::raw_auth_config`] (pacquet's stand-ins for pnpm's
//! `explicitlySetKeys` + `authConfig`).

mod ini;

#[cfg(test)]
mod tests;

use clap::{Args, Subcommand, ValueEnum};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_config::{
    Config, GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME, config_types, naming_cases,
    property_path::{self, Segment},
    protected_settings,
};
use pacquet_workspace_manifest_writer::update_manifest_field;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Manage the pnpm configuration files.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[clap(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Set the config key to the value provided.
    Set(ConfigSetArgs),
    /// Print the config value for the provided key.
    Get(ConfigGetArgs),
    /// Remove the config key from the config file.
    Delete(ConfigDeleteArgs),
    /// Show all the config settings.
    List(ConfigListArgs),
}

/// Flags shared by the `config` subcommands.
#[derive(Debug, Default, Clone, Copy, Args)]
pub struct ConfigFlags {
    /// Operate on the global config file.
    #[clap(short = 'g', long, global = true)]
    pub global: bool,

    /// Which config to read or write: `project` for the project's config,
    /// `global` for the global config.
    #[clap(long, value_enum, global = true)]
    pub location: Option<ConfigLocation>,

    /// Show all types of values in JSON format (not just objects and arrays).
    #[clap(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConfigLocation {
    Project,
    Global,
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    pub key: Option<String>,
    pub value: Option<String>,
    #[clap(flatten)]
    pub flags: ConfigFlags,
}

#[derive(Debug, Args)]
pub struct ConfigGetArgs {
    pub key: Option<String>,
    #[clap(flatten)]
    pub flags: ConfigFlags,
}

#[derive(Debug, Args)]
pub struct ConfigDeleteArgs {
    pub key: Option<String>,
    #[clap(flatten)]
    pub flags: ConfigFlags,
}

#[derive(Debug, Args)]
pub struct ConfigListArgs {
    #[clap(flatten)]
    pub flags: ConfigFlags,
}

/// Errors raised by `pacquet config`, mirroring the `PnpmError` codes pnpm's
/// config command raises (the `ERR_PNPM_` prefix is part of the public
/// contract; see <https://pnpm.io/errors>).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ConfigError {
    #[display("`pacquet config {subcommand}` requires the config key")]
    #[diagnostic(code(ERR_PNPM_CONFIG_NO_PARAMS))]
    NoParams { subcommand: String },

    #[display("Cannot set {key} to a non-string value ({value})")]
    #[diagnostic(code(ERR_PNPM_CONFIG_SET_AUTH_NON_STRING))]
    SetAuthNonString { key: String, value: String },

    #[display("Cannot set config with an empty key")]
    #[diagnostic(code(ERR_PNPM_CONFIG_SET_EMPTY_KEY))]
    SetEmptyKey,

    #[display("Setting deep property path is not supported")]
    #[diagnostic(code(ERR_PNPM_CONFIG_SET_DEEP_KEY))]
    SetDeepKey,

    #[display("Key {key:?} isn't supported by INI config files")]
    #[diagnostic(
        code(ERR_PNPM_CONFIG_SET_UNSUPPORTED_INI_CONFIG_KEY),
        help("Add {camel:?} to the project workspace manifest instead")
    )]
    SetUnsupportedIniConfigKey { key: String, camel: String },

    #[display("The key {key:?} isn't supported by the workspace manifest")]
    #[diagnostic(code(ERR_PNPM_CONFIG_SET_UNSUPPORTED_WORKSPACE_KEY), help("Try {camel:?}"))]
    SetUnsupportedWorkspaceKey { key: String, camel: String },

    #[display("The key {key:?} isn't supported by the global config.yaml file")]
    #[diagnostic(
        code(ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY),
        help("Try setting them instead to the local pnpm-workspace.yaml file")
    )]
    SetUnsupportedYamlConfigKey { key: String },

    #[display("Invalid property path: {_0}")]
    #[diagnostic(code(ERR_PNPM_CONFIG_INVALID_PROPERTY_PATH))]
    InvalidPropertyPath(#[error(not(source))] property_path::ParsePropertyPathError),

    #[display("Invalid JSON value: {_0}")]
    #[diagnostic(code(ERR_PNPM_CONFIG_INVALID_JSON))]
    InvalidJson(#[error(not(source))] serde_json::Error),

    #[display("The global config directory could not be determined")]
    #[diagnostic(code(ERR_PNPM_CONFIG_NO_GLOBAL_DIR))]
    NoGlobalConfigDir,

    // The rejected value is deliberately not echoed — it may be a credential
    // (e.g. a token with a stray newline pasted from an env var).
    #[display("Cannot write a value containing a control character to an INI config file")]
    #[diagnostic(code(pacquet_cli::config_set_invalid_control_character))]
    SetIniControlCharacter,
}

impl ConfigArgs {
    pub fn run(self, config: &Config, dir: &Path) -> miette::Result<()> {
        match self.command {
            ConfigSubcommand::Set(args) => {
                let flags = args.flags;
                let (key, value) = split_set_params(args.key, args.value, "set")?;
                config_set(config, dir, flags, &key, Some(value))?;
            }
            ConfigSubcommand::Delete(args) => {
                let key = args
                    .key
                    .filter(|key| !key.is_empty())
                    .ok_or_else(|| ConfigError::NoParams { subcommand: "delete".to_string() })?;
                config_set(config, dir, args.flags, &key, None)?;
            }
            ConfigSubcommand::Get(args) => {
                let output = match args.key.as_deref().filter(|key| !key.is_empty()) {
                    Some(key) => config_get(config, args.flags, key)?,
                    None => config_list(config),
                };
                println!("{output}");
            }
            ConfigSubcommand::List(args) => {
                let _ = args.flags;
                println!("{}", config_list(config));
            }
        }
        Ok(())
    }
}

/// Resolve the effective `global` boolean from the `--location` / `--global`
/// flags. Mirrors pnpm's handler: `--location` wins, otherwise config
/// operations default to global.
fn resolve_global(flags: ConfigFlags) -> bool {
    match flags.location {
        Some(ConfigLocation::Global) => true,
        Some(ConfigLocation::Project) => false,
        // No `--location`: pnpm defaults config operations to global when no
        // explicit location was given (a bare `--global` lands here too).
        None => true,
    }
}

/// Split `pnpm config set <key> [value]` params, handling the `key=value` form
/// when no separate value is given. Mirrors the `set` arm of pnpm's handler.
fn split_set_params(
    key: Option<String>,
    value: Option<String>,
    subcommand: &str,
) -> Result<(String, String), ConfigError> {
    let key = key
        .filter(|key| !key.is_empty())
        .ok_or_else(|| ConfigError::NoParams { subcommand: subcommand.to_string() })?;
    match value {
        Some(value) => Ok((key, value)),
        None => {
            // `key=value` form: the key is everything before the first `=`, the
            // value everything after (so a value may itself contain `=`).
            match key.split_once('=') {
                Some((k, v)) => Ok((k.to_string(), v.to_string())),
                None => Ok((key, String::new())),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// config set / delete
// ---------------------------------------------------------------------------

/// `pnpm config set` (when `value` is `Some`) / `pnpm config delete` (when
/// `value` is `None`). Port of `configSet`.
fn config_set(
    config: &Config,
    dir: &Path,
    flags: ConfigFlags,
    key: &str,
    value: Option<String>,
) -> miette::Result<()> {
    let global = resolve_global(flags);
    let mut key = key.to_string();
    let mut is_auth_setting = config_types::is_ini_config_key(&key);
    if !is_auth_setting {
        key = validate_simple_key(&key)?;
        is_auth_setting = config_types::is_ini_config_key(&key);
    }

    // The cast/parsed value. `None` (delete) and JSON `null` both delete.
    let value: Value = match value {
        None => Value::Null,
        Some(raw) if flags.json => serde_json::from_str(&raw).map_err(ConfigError::InvalidJson)?,
        Some(raw) => Value::String(raw),
    };

    if is_auth_setting {
        let config_path =
            if global { global_config_dir(config)?.join("auth.ini") } else { dir.join(".npmrc") };
        if !value.is_null() && !value.is_string() && is_string_only_ini_key(&key) {
            return Err(ConfigError::SetAuthNonString { key, value: value.to_string() }.into());
        }
        return write_ini_setting(&config_path, &key, &value);
    }

    let (config_dir, config_file_name) = get_config_file_info(&key, global, config, dir)?;
    let config_path = config_dir.join(config_file_name);

    match config_file_name {
        GLOBAL_CONFIG_YAML_FILENAME | WORKSPACE_MANIFEST_FILENAME => {
            if config_file_name == GLOBAL_CONFIG_YAML_FILENAME {
                key = validate_yaml_config_key(&key)?;
            }
            key = validate_workspace_key(&key)?;
            let cast = cast_field(value, &naming_cases::to_kebab_case(&key));
            update_manifest_field(&config_path, &key, &cast).map_err(miette::Report::new)?;
        }
        _ => {
            // INI file reached via `getConfigFileInfo` (auth/scoped/registry key
            // whose kebab-case form is an INI key). Validate against `types`.
            key = validate_ini_config_key(&key)?;
            write_ini_setting(&config_path, &key, &value)?;
        }
    }
    Ok(())
}

/// Read the INI file, set or delete `key`, and write it back. A delete of an
/// absent key is a no-op (no write). Mirrors the INI arms of `configSet`.
fn write_ini_setting(config_path: &Path, key: &str, value: &Value) -> miette::Result<()> {
    let mut settings = ini::read(config_path)
        .map_err(miette::Report::msg)
        .map_err(|err| err.wrap_err(format!("reading {}", config_path.display())))?;
    if value.is_null() {
        if settings.shift_remove(key).is_none() {
            return Ok(());
        }
    } else {
        let value_string = ini_value_string(value);
        // A control character (notably a newline) in the value would split into
        // extra `key=value` lines when the INI file is re-parsed, injecting
        // settings the user never set. Refuse rather than corrupt the file.
        if has_control_char(key) || has_control_char(&value_string) {
            return Err(ConfigError::SetIniControlCharacter.into());
        }
        settings.insert(key.to_string(), value_string);
    }
    ini::write(config_path, &settings)
        .map_err(miette::Report::msg)
        .map_err(|err| err.wrap_err(format!("writing {}", config_path.display())))?;
    Ok(())
}

/// Whether `text` holds a control character. The INI writer splices `text`
/// into a single `key=value` line, so a control character would corrupt the
/// file; the values `config set` writes never legitimately contain one.
fn has_control_char(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// Render a JSON value as its INI string form. Auth values are strings; the
/// non-string forms (only reachable for keys that are not string-only) follow
/// the `ini` package's scalar stringification.
fn ini_value_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `getConfigFileInfo`: route `key` to its config directory and file name.
fn get_config_file_info<'a>(
    key: &str,
    global: bool,
    config: &'a Config,
    dir: &'a Path,
) -> Result<(PathBuf, &'static str), ConfigError> {
    let kebab = naming_cases::to_kebab_case(key);
    let config_dir = if global { global_config_dir(config)? } else { dir.to_path_buf() };
    let file_name = if config_types::is_ini_config_key(&kebab) {
        if global { "auth.ini" } else { ".npmrc" }
    } else if global {
        GLOBAL_CONFIG_YAML_FILENAME
    } else {
        WORKSPACE_MANIFEST_FILENAME
    };
    Ok((config_dir, file_name))
}

fn global_config_dir(config: &Config) -> Result<PathBuf, ConfigError> {
    config.config_dir.clone().ok_or(ConfigError::NoGlobalConfigDir)
}

/// `castField`: coerce a string value per its key's type. Booleans, `null`,
/// and `undefined` literals are recognized; numeric-typed keys parse to a
/// number; everything else is the trimmed string. Non-string values pass
/// through unchanged. `undefined` maps to JSON `null` (a deletion downstream).
fn cast_field(value: Value, kebab_key: &str) -> Value {
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
fn validate_simple_key(key: &str) -> Result<String, ConfigError> {
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
fn validate_workspace_key(key: &str) -> Result<String, ConfigError> {
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
fn validate_ini_config_key(key: &str) -> Result<String, ConfigError> {
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
fn validate_yaml_config_key(key: &str) -> Result<String, ConfigError> {
    let kebab = naming_cases::to_kebab_case(key);
    if config_types::is_config_file_key(&kebab) {
        return Ok(kebab);
    }
    Err(ConfigError::SetUnsupportedYamlConfigKey { key: key.to_string() })
}

const STRING_ONLY_INI_KEYS: &[&str] = &["_auth", "_authToken", "_password", "username", "registry"];

fn is_string_only_ini_key(key: &str) -> bool {
    STRING_ONLY_INI_KEYS.contains(&key) || key.starts_with('@') || key.starts_with("//")
}

// ---------------------------------------------------------------------------
// config get / list
// ---------------------------------------------------------------------------

/// `configGet`: resolve and render the value at `key`. Port of `configGet`.
fn config_get(config: &Config, flags: ConfigFlags, key: &str) -> Result<String, ConfigError> {
    let is_scoped = key.starts_with('@');
    let value = match lookup_config(config, key, is_scoped) {
        Some(value) => value,
        None if is_property_path(key) => lookup_by_property_path(config, key)?,
        None => Value::Null,
    };
    Ok(display_config(&value, flags.json))
}

/// `configList`: the full config record as pretty JSON. Port of `configList`.
fn config_list(config: &Config) -> String {
    serde_json::to_string_pretty(&Value::Object(config_to_record(config)))
        .expect("serializing the config record to JSON never fails")
}

/// `lookupConfig`: resolve `key` against scoped registries, `globalconfig`,
/// the typed settings, the raw auth keys, or the config record. `None` means
/// "not found, fall through to a property-path lookup".
fn lookup_config(config: &Config, key: &str, is_scoped: bool) -> Option<Value> {
    if is_scoped {
        if let Some(scope) = key.strip_suffix(":registry") {
            // Prefer the merged `registries` map so this reports the same URL
            // resolvers/publish use (pnpm/pnpm#11492).
            if let Some(merged) = config.registries.get(scope) {
                return Some(Value::String(merged.clone()));
            }
        }
        return Some(auth_value(config, key));
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
    if config_types::is_type_key(&kebab) {
        let camel = naming_cases::to_camel_case(&kebab);
        if let Some(value) = config.explicit_settings.get(&camel) {
            return Some(value.clone());
        }
        if let Some(value) = config.raw_auth_config.get(&kebab) {
            return Some(Value::String(value.clone()));
        }
        return Some(Value::Null);
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
fn config_to_record(config: &Config) -> Map<String, Value> {
    let mut result: Map<String, Value> = Map::new();
    for (key, value) in &config.explicit_settings {
        result.insert(key.clone(), value.clone());
    }
    for (key, value) in &config.raw_auth_config {
        result.entry(key.clone()).or_insert_with(|| Value::String(value.clone()));
    }
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
