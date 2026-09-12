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
use pnpm_config::{
    Config, DEFAULT_JSR_REGISTRY, GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME,
    config_types, naming_cases, property_path, property_path::Segment, protected_settings,
};
use pnpm_workspace_manifest_writer::update_manifest_field;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use values::{
    cast_field, config_get, config_list, is_string_only_ini_key, validate_ini_config_key,
    validate_simple_key, validate_workspace_key, validate_yaml_config_key,
};

/// Manage the pnpm configuration files.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[clap(flatten)]
    pub flags: ConfigFlags,

    #[clap(subcommand)]
    pub command: ConfigSubcommand,
}

impl ConfigArgs {
    /// Whether `--global` / `-g` was passed, ignoring the default
    /// [`resolve_global`] applies when no location flag is given.
    pub(super) fn is_global(&self) -> bool {
        self.flags.global
    }
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
}

#[derive(Debug, Args)]
pub struct ConfigGetArgs {
    pub key: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigDeleteArgs {
    pub key: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigListArgs {}

#[derive(Debug, Args)]
pub struct ConfigSetAliasArgs {
    #[clap(flatten)]
    pub args: ConfigSetArgs,

    #[clap(flatten)]
    pub flags: ConfigFlags,
}

#[derive(Debug, Args)]
pub struct ConfigGetAliasArgs {
    #[clap(flatten)]
    pub args: ConfigGetArgs,

    #[clap(flatten)]
    pub flags: ConfigFlags,
}

/// Errors raised by `pacquet config`, mirroring the `PnpmError` codes pnpm's
/// config command raises (the `ERR_PNPM_` prefix is part of the public
/// contract; see <https://pnpm.io/errors>).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ConfigError {
    #[display("`pnpm config {subcommand}` requires the config key")]
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
    #[diagnostic(code(ERR_PNPM_CLI_CONFIG_SET_INVALID_CONTROL_CHARACTER))]
    SetIniControlCharacter,
}

impl ConfigArgs {
    pub fn run(self, config: &Config, dir: &Path) -> miette::Result<()> {
        let Self { flags, command } = self;
        match command {
            ConfigSubcommand::Set(args) => {
                let (key, value) = split_set_params(args.key, args.value, "set")?;
                config_set(config, dir, flags, &key, Some(value))?;
            }
            ConfigSubcommand::Delete(args) => {
                let key = args
                    .key
                    .filter(|key| !key.is_empty())
                    .ok_or_else(|| ConfigError::NoParams { subcommand: "delete".to_string() })?;
                config_set(config, dir, flags, &key, None)?;
            }
            ConfigSubcommand::Get(args) => {
                let output = match args.key.as_deref().filter(|key| !key.is_empty()) {
                    Some(key) => config_get(config, flags, key)?,
                    None => config_list(config),
                };
                println!("{output}");
            }
            ConfigSubcommand::List(_) => {
                println!("{}", config_list(config));
            }
        }
        Ok(())
    }
}

/// Resolve the effective `global` boolean from the `--location` / `--global`
/// flags. Mirrors pnpm's handler: `--location` wins, otherwise config
/// operations default to global.
pub(super) fn resolve_global(flags: ConfigFlags) -> bool {
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
        return set_auth_setting(config, dir, global, key, &value);
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

/// Write an auth setting to the global `auth.ini` or the directory's
/// `.npmrc`.
fn set_auth_setting(
    config: &Config,
    dir: &Path,
    global: bool,
    key: String,
    value: &Value,
) -> miette::Result<()> {
    let config_path =
        if global { global_config_dir(config)?.join("auth.ini") } else { dir.join(".npmrc") };
    if !value.is_null() && !value.is_string() && is_string_only_ini_key(&key) {
        return Err(ConfigError::SetAuthNonString { key, value: value.to_string() }.into());
    }
    write_ini_setting(&config_path, &key, value)
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

mod values;
