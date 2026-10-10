//! The config dependencies `pnpm update` updates.

use super::UpdateArgs;
use crate::config_deps;
use pnpm_config::Config;
use pnpm_env_installer::ConfigDepUpdates;
use pnpm_matcher::create_matcher;
use pnpm_package_manager::parse_update_param;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::UpdateBehavior;
use std::{collections::BTreeMap, path::Path};

impl UpdateArgs {
    /// Update the config dependencies [`Self::config_dependency_updates`]
    /// selects, before the install reads them.
    pub(crate) async fn update_config_dependencies<Reporter: self::Reporter>(
        &self,
        config: &mut Config,
        config_root: &Path,
    ) -> miette::Result<()> {
        // The options the update itself rejects later must not have
        // written anything by then.
        self.check_patches_options()?;
        self.check_interactive_peer_options()?;
        self.check_workspace_option(config.workspace_dir.as_deref())?;
        let updates = self.config_dependency_updates(config);
        config_deps::update_config_dependencies::<Reporter>(config, config_root, &updates).await
    }

    /// The config dependencies this update resolves again: those the
    /// selectors name, or every one when there are none. Each keeps the
    /// declared specifier's operator, as a project dependency does.
    ///
    /// None with `--interactive`, `--patches`, `--no-save`, `--filter`, or a
    /// flag that narrows the dependency groups: each of those scopes the
    /// update to project dependencies. A versioned selector names a
    /// project dependency.
    fn config_dependency_updates(&self, config: &Config) -> ConfigDepUpdates {
        let behavior =
            if self.selection.latest { UpdateBehavior::Latest } else { UpdateBehavior::Compatible };
        let mut updates = ConfigDepUpdates {
            prev_specifiers: BTreeMap::new(),
            behavior,
            range_spec_style: self.range_spec_style(config),
        };
        let groups = &self.dependency_options;
        let narrowed = self.selection.interactive
            || self.selection.patches
            || self.save.no_save
            || !config.filter.is_empty()
            || !config.filter_prod.is_empty()
            || groups.prod
            || groups.dev
            || groups.optional
            || groups.no_optional;
        let Some(declared) = config.config_dependencies.as_ref().filter(|_| !narrowed) else {
            return updates;
        };
        let patterns = self.packages
            .iter()
            .map(|package| parse_update_param(package))
            .filter(|selector| selector.version.is_none())
            .map(|selector| selector.pattern)
            .collect::<Vec<_>>();
        if patterns.is_empty() && !self.packages.is_empty() {
            return updates;
        }
        let matcher = create_matcher(&patterns);
        updates.prev_specifiers = declared
            .keys()
            .filter(|name| patterns.is_empty() || matcher.matches(name))
            .filter_map(|name| {
                let specifier = config_deps::declared_specifier(declared, name)?;
                Some((name.clone(), Some(specifier)))
            })
            .collect();
        updates
    }
}
