use crate::Config;
use pnpm_network::normalize_registry_url;
use std::collections::BTreeMap;

/// The settings the command line sets, which outrank every other source.
/// The CLI seeds it before [`Config::current`], which reads it as the top
/// layer of each source kind it carries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CliLayer {
    /// Every setting the command line set, as given and in argv order:
    /// each `--config.<key>=<value>` token and each install flag such as
    /// `--offline` or `--store-dir`, keyed by its kebab-case setting name.
    /// [`Config::current`] reads it as the top `.npmrc` layer, and it is
    /// forwarded to a child pnpm the command spawns.
    pub npmrc_tokens: Vec<(String, String)>,

    /// The default registry: `--registry`, else the last
    /// `--config.registry`. Unscoped credentials of
    /// [`Self::npmrc_tokens`] pin to it.
    pub registry: Option<String>,

    /// The `--config.@<scope>:registry` routes, keyed by `@<scope>`.
    pub scoped_registries: BTreeMap<String, String>,

    /// The values of settings the CLI has no handling of its own for,
    /// keyed by kebab-case name. [`Config::current`] applies them above
    /// `PNPM_CONFIG_*` and before the derivations that read the final
    /// settings, such as the lockfile-dir anchoring and the global virtual
    /// store.
    pub setting_values: BTreeMap<String, String>,
}

impl Config {
    /// Point the default registry at `registry`, in the named-registry map
    /// and the package-manager bootstrap too, and record it as set on the
    /// command line. The URL is normalized to a trailing slash first, so a
    /// normalized override applies idempotently.
    pub fn apply_registry_override(&mut self, registry: &str) {
        let registry = normalize_registry_url(registry).into_owned();
        self.registry.clone_from(&registry);
        self.registries_by_scope.insert("default".to_string(), registry.clone());
        self.package_manager_bootstrap.registry.clone_from(&registry);
        self.package_manager_bootstrap.registries.insert("default".to_string(), registry);
        self.cli_settings.insert("registry".to_string());
    }

    /// Apply the [`CliLayer`] registry routes over every other source.
    /// Callers that change registries after the load, such as the
    /// `updateConfig` hooks, call it again to keep the command line on top.
    pub fn apply_cli_registries(&mut self) {
        if let Some(registry) = self.cli.registry.clone() {
            self.apply_registry_override(&registry);
        }
        for (scope, registry) in &self.cli.scoped_registries {
            self.registries_by_scope.insert(scope.clone(), registry.clone());
            self.package_manager_bootstrap.registries.insert(scope.clone(), registry.clone());
        }
    }
}
