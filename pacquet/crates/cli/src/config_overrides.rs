use crate::cli_args::CliArgs;
use clap::CommandFactory;
use pacquet_config::{Config, GetHomeDir, Host};
use pacquet_fs::lexical_normalize;
use pacquet_store_dir::StoreDir;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::Path,
};

pub(crate) fn apply_store_dir_override<Sys: GetHomeDir>(
    config: &mut Config,
    store_dir: &Path,
    dir: &Path,
) -> miette::Result<()> {
    let workspace_dir = config.workspace_dir.as_deref().unwrap_or(dir).to_path_buf();
    if store_dir.as_os_str().is_empty() {
        config.reset_store_dir_to_default::<Host>(&workspace_dir);
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(String::new()));
        return Ok(());
    }
    let resolved = if let Some(relative) = home_relative_store_dir(store_dir) {
        Sys::home_dir()
            .ok_or_else(|| {
                let store_dir_display = store_dir.display();
                miette::miette!(
                    "Cannot resolve store directory {} because the home directory is unknown",
                    store_dir_display,
                )
            })?
            .join(relative)
    } else if store_dir.is_absolute() {
        store_dir.to_path_buf()
    } else {
        workspace_dir.join(store_dir)
    };
    config.store_dir = StoreDir::from(lexical_normalize(&resolved));
    if let Some(store_dir) = store_dir.to_str() {
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(store_dir.to_string()));
    }
    let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
    let global_virtual_store_dir_explicit =
        config.explicit_settings.contains_key("globalVirtualStoreDir");
    config.apply_global_virtual_store_derivation(
        virtual_store_dir_explicit,
        global_virtual_store_dir_explicit,
    );
    Ok(())
}

fn home_relative_store_dir(store_dir: &Path) -> Option<&Path> {
    let store_dir = store_dir.to_str()?;
    store_dir.strip_prefix("~/").or_else(|| store_dir.strip_prefix(r"~\")).map(Path::new)
}

/// CLI overrides parsed from pnpm's `--config.<key>=<value>` dotted-key
/// syntax. Upstream pnpm uses [`npm-conf`](https://github.com/npm/npm-conf)
/// to translate each `--config.<key>=<value>` token into a runtime config
/// assignment that wins over `.npmrc` and `pnpm-workspace.yaml`; pacquet
/// mirrors that by stripping the same tokens out of argv before clap sees
/// them and re-applying them onto [`Config`] after the file-based layers
/// have run.
///
/// Unknown keys are accepted silently: pnpm exposes a long tail of config
/// keys, and erroring on an unrecognized one would break the moment pnpm
/// adds a new key that pacquet hasn't ported yet. The token has already
/// been honored by pnpm itself before delegation, so dropping it on
/// pacquet's side just means the pacquet leg falls back to the yaml/npmrc
/// value — never an incorrect override.
#[derive(Debug, Default)]
pub struct ConfigOverrides {
    registry: Option<String>,
    registries: BTreeMap<String, String>,
    deploy_all_files: Option<bool>,
    force_legacy_deploy: Option<bool>,
    shared_workspace_lockfile: Option<bool>,
}

impl ConfigOverrides {
    /// Pull `--config.<key>=<value>` tokens out of `argv` and collect
    /// them. Returns the parsed overrides together with the remaining
    /// argv tokens (in their original order) for clap to parse.
    pub fn extract<Argv>(argv: Argv) -> (Self, Vec<OsString>)
    where
        Argv: IntoIterator<Item = OsString>,
    {
        let argv = argv.into_iter().collect::<Vec<_>>();
        let external_command_index = external_command_index(&argv);
        let mut overrides = Self::default();
        let mut remaining = Vec::new();
        for (index, arg) in argv.into_iter().enumerate() {
            if external_command_index.is_some_and(|command_index| index > command_index) {
                remaining.push(arg);
                continue;
            }
            match classify(&arg) {
                ConfigToken::WellFormed { key: "store-dir", value } => {
                    remaining.push(OsString::from(format!("--store-dir={value}")));
                }
                ConfigToken::WellFormed { key, value } => overrides.set(key, value),
                ConfigToken::Malformed => {}
                ConfigToken::NotOurs => remaining.push(arg),
            }
        }
        (overrides, remaining)
    }

    fn set(&mut self, key: &str, value: &str) {
        if key == "registry" {
            self.registry = Some(normalize_registry_url(value));
            return;
        }
        if key == "deploy-all-files" {
            self.deploy_all_files = parse_bool(value);
            return;
        }
        if key == "force-legacy-deploy" {
            self.force_legacy_deploy = parse_bool(value);
            return;
        }
        if key == "shared-workspace-lockfile" {
            self.shared_workspace_lockfile = parse_bool(value);
            return;
        }
        if let Some(scope) = scoped_registry_key(key) {
            self.registries.insert(scope.to_owned(), normalize_registry_url(value));
        }
    }

    /// Layer the CLI overrides on top of a [`Config`] that has already
    /// been built from defaults, `.npmrc`, and `pnpm-workspace.yaml`.
    /// Mirrors pnpm 11's "CLI > yaml > .npmrc > defaults" precedence.
    pub fn apply(&self, config: &mut Config) {
        if let Some(registry) = &self.registry {
            config.registry.clone_from(registry);
            config.registries.insert("default".to_string(), registry.clone());
            config.package_manager_bootstrap.registry.clone_from(registry);
            config
                .package_manager_bootstrap
                .registries
                .insert("default".to_string(), registry.clone());
        }
        for (scope, registry) in &self.registries {
            config.registries.insert(scope.clone(), registry.clone());
            config.package_manager_bootstrap.registries.insert(scope.clone(), registry.clone());
        }
        if let Some(value) = self.deploy_all_files {
            config.deploy_all_files = value;
        }
        if let Some(value) = self.force_legacy_deploy {
            config.force_legacy_deploy = value;
        }
        if let Some(value) = self.shared_workspace_lockfile {
            config.shared_workspace_lockfile = value;
        }
    }
}

fn external_command_index(argv: &[OsString]) -> Option<usize> {
    let mut index = 1;
    while index < argv.len() {
        let Some(arg) = argv[index].to_str() else {
            return Some(index);
        };
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--config.") {
            index += 1;
            continue;
        }
        if let Some(width) = global_option_width(arg) {
            index += width;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return (!is_known_top_level_command(arg)).then_some(index);
    }
    None
}

fn global_option_width(arg: &str) -> Option<usize> {
    if matches!(arg, "-r" | "-v") {
        return Some(1);
    }
    if matches!(arg, "-C" | "-F") {
        return Some(2);
    }
    if arg.starts_with("-C") || arg.starts_with("-F") {
        return Some(1);
    }
    let name = arg.strip_prefix("--")?;
    let (name, has_value) = name.split_once('=').map_or((name, false), |(name, _)| (name, true));
    let consumes_value = matches!(
        name,
        "dir"
            | "filter"
            | "filter-prod"
            | "npmrc-auth-file"
            | "reporter"
            | "store-dir"
            | "userconfig",
    );
    Some(if consumes_value && !has_value { 2 } else { 1 })
}

fn is_known_top_level_command(name: &str) -> bool {
    CliArgs::command().get_subcommands().any(|command| {
        command.get_name() == name || command.get_all_aliases().any(|alias| alias == name)
    })
}

enum ConfigToken<'a> {
    WellFormed { key: &'a str, value: &'a str },
    Malformed,
    NotOurs,
}

/// Decide whether an argv token belongs to the `--config.<key>=<value>`
/// family. Everything with a `--config.` prefix is claimed, so a typo
/// like `--config.foo` never escapes into clap's "unexpected argument"
/// path; non-prefixed tokens are returned untouched.
fn classify(arg: &OsStr) -> ConfigToken<'_> {
    let Some(rest) = arg.to_str().and_then(|arg| arg.strip_prefix("--config.")) else {
        return ConfigToken::NotOurs;
    };
    let Some((key, value)) = rest.split_once('=') else {
        return ConfigToken::Malformed;
    };
    if key.is_empty() {
        return ConfigToken::Malformed;
    }
    ConfigToken::WellFormed { key, value }
}

fn scoped_registry_key(key: &str) -> Option<&str> {
    key.strip_suffix(":registry")
        .filter(|scope| scope.starts_with('@') && scope.len() > 1 && !scope.contains('/'))
}

fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
