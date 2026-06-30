use pacquet_config::Config;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

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
        let mut overrides = Self::default();
        let mut remaining = Vec::new();
        for arg in argv {
            match classify(&arg) {
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
