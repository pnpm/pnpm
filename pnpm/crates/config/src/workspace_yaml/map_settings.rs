use super::WorkspaceSettings;
use pnpm_workspace::map_settings::{
    CatalogCleanup,
    DefaultCatalog,
    MapSetting,
    Rules,
    WarningPolicy,
};
use serde_json::{
    Map,
    Value,
};

fn warn_unused<MapType>(rules: Option<&Rules<MapType>>, alias: Option<bool>) -> Option<bool> {
    rules
        .and_then(MapSetting::section)
        .and_then(|section| section.unused)
        .map(|mode| mode == WarningPolicy::Warn)
        .or(alias)
}

impl WorkspaceSettings {
    fn prune_catalog(&self) -> Option<bool> {
        self.catalog
            .as_ref()
            .and_then(MapSetting::section)
            .and_then(|section| section.unused)
            .map(|mode| mode == CatalogCleanup::Prune)
            .or(self.catalog_prune)
            .or(self.cleanup_unused_catalogs)
    }

    /// A config dependency's settings are compared key by key against the resolved
    /// settings and the environment, using the same keys for both spellings.
    pub(super) fn flatten_package_maps(&mut self) {
        self.warn_unused_permissions =
            warn_unused(self.permissions.as_ref(), self.warn_unused_permissions);
        self.warn_unused_overrides =
            warn_unused(self.overrides.as_ref(), self.warn_unused_overrides);
        self.catalog_prune = self.prune_catalog();
        self.permissions = self.permissions
            .take()
            .map(Rules::into_rules)
            .map(Rules::Flat);
        self.overrides = self.overrides
            .take()
            .map(Rules::into_rules)
            .map(Rules::Flat);
        self.catalog = self.catalog
            .take()
            .map(DefaultCatalog::into_entries)
            .map(DefaultCatalog::Flat);
    }

    pub(crate) fn mirror_map_policies(&self, target: &mut Map<String, Value>) {
        for (name, marker, key, mode, enabled) in [
            (
                "permissions",
                "rules",
                "warnUnusedPermissions",
                "warn",
                warn_unused(self.permissions.as_ref(), self.warn_unused_permissions),
            ),
            (
                "overrides",
                "rules",
                "warnUnusedOverrides",
                "warn",
                warn_unused(self.overrides.as_ref(), self.warn_unused_overrides),
            ),
            ("catalog", "entries", "catalogPrune", "prune", self.prune_catalog()),
        ] {
            let Some(enabled) = enabled else { continue };
            target.insert(key.to_owned(), Value::Bool(enabled));
            if let Some(unused) =
                target.get_mut(name).and_then(|section| explicit_policy(section, marker))
            {
                *unused = Value::from(if enabled { mode } else { "ignore" });
            }
        }
    }
}

fn explicit_policy<'a>(section: &'a mut Value, marker: &str) -> Option<&'a mut Value> {
    section.get(marker)?.as_object()?;
    section.get_mut("unused").filter(|value| value.is_string())
}
