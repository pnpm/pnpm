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
        self.warn_unused_overrides =
            warn_unused(self.overrides.as_ref(), self.warn_unused_overrides);
        self.catalog_prune = self.prune_catalog();
        self.overrides = self.overrides
            .take()
            .map(Rules::into_rules)
            .map(Rules::Flat);
        if let Some(DefaultCatalog::Section(section)) = &mut self.catalog {
            super::overlay_some(&mut self.catalog_mode, section.mode);
            super::overlay_some(&mut self.save_catalog_name, section.default_save_target.take());
        }
        self.catalog = self.catalog
            .take()
            .map(DefaultCatalog::into_entries)
            .map(DefaultCatalog::Flat);
    }

    fn mirror_catalog_options(&self, target: &mut Map<String, Value>) {
        let section = self.catalog.as_ref().and_then(MapSetting::section);
        let mode = section.and_then(|section| section.mode).or(self.catalog_mode);
        let default_save_target = section
            .and_then(|section| section.default_save_target.as_ref())
            .or(self.save_catalog_name.as_ref());
        for (field, alias, value) in [
            ("mode", "catalogMode", mode.map(|mode| serde_json::json!(mode))),
            ("defaultSaveTarget", "saveCatalogName", default_save_target.cloned().map(Value::from)),
        ] {
            if let Some(value) = value {
                if let Some(nested) = target
                    .get_mut("catalog")
                    .and_then(|catalog| explicit_option(catalog, "entries", field))
                {
                    nested.clone_from(&value);
                }
                target.insert(alias.to_owned(), value);
            }
        }
    }

    pub(crate) fn mirror_map_policies(&self, target: &mut Map<String, Value>) {
        self.mirror_catalog_options(target);
        for (name, marker, key, mode, enabled) in [
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
                target.get_mut(name).and_then(|section| explicit_option(section, marker, "unused"))
            {
                *unused = Value::from(if enabled { mode } else { "ignore" });
            }
        }
    }
}

fn explicit_option<'a>(section: &'a mut Value, marker: &str, field: &str) -> Option<&'a mut Value> {
    section.get(marker)?.as_object()?;
    section.get_mut(field).filter(|value| value.is_string())
}
