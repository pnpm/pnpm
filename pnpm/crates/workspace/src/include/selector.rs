use super::{
    Delegation, INCLUDE_KEY, Include, IncludeError, IncludeSource, SETTINGS_FORBIDDEN_TO_PACKAGES,
    UNRESTRICTED,
};
use std::{fmt, path::Path};
use yaml_serde::{Mapping, Value};

/// The unit of ownership: a setting, or one catalog.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SettingSlot {
    /// A top-level setting, by its canonical name.
    Setting(String),
    /// One catalog, by name. The default catalog is `default`.
    Catalog(String),
    /// Every catalog, including those added later. Only a delegation reserves
    /// this slot.
    AllCatalogs,
}

impl SettingSlot {
    /// The slot a selector of a setting list names.
    fn parse_selector(selector: &str) -> Option<Self> {
        match selector {
            "" | INCLUDE_KEY => None,
            "catalog" => Some(SettingSlot::Catalog(DEFAULT_CATALOG.to_string())),
            "catalogs" => Some(SettingSlot::AllCatalogs),
            _ => match selector.strip_prefix("catalogs.") {
                Some("") => None,
                Some(name) => Some(SettingSlot::Catalog(name.to_string())),
                None if selector.contains('.') => None,
                None => Some(SettingSlot::Setting(canonical_setting_name(selector).to_string())),
            },
        }
    }

    /// Does a delegation of `self` permit defining `slot`?
    #[must_use]
    pub fn covers(&self, slot: &SettingSlot) -> bool {
        self == slot || matches!((self, slot), (SettingSlot::AllCatalogs, SettingSlot::Catalog(_)))
    }
}

impl fmt::Display for SettingSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingSlot::Setting(name) => formatter.write_str(name),
            SettingSlot::Catalog(name) if name == DEFAULT_CATALOG => formatter.write_str("catalog"),
            SettingSlot::Catalog(name) => write!(formatter, "catalogs.{name}"),
            SettingSlot::AllCatalogs => formatter.write_str("catalogs"),
        }
    }
}

const DEFAULT_CATALOG: &str = "default";

/// Not a setting: an editor's schema association.
const SCHEMA_DIRECTIVE_KEY: &str = "$schema";

/// The setting a spelling stands for. Spellings of one setting share one
/// owner, so a legacy spelling cannot evade a delegation.
#[must_use]
pub fn canonical_setting_name(key: &str) -> &str {
    match key {
        "enableGlobalVirtualStore" => "virtualStoreType",
        "nodeExperimentalPackageMap" | "nodePackageMapType" => "nodePackageMap",
        "noproxy" => "noProxy",
        "proxy" => "httpsProxy",
        // A package's `build` permission outranks its `allowBuilds` entry.
        "permissions" => "allowBuilds",
        "auditConfig" | "auditLevel" => "audit",
        "cleanupUnusedCatalogs" => "catalogPrune",
        "maxsockets" => "maxSockets",
        "namedRegistries" => "registries",
        "updateConfig" => "update",
        _ => key,
    }
}

/// The slots a top-level key of a settings file defines.
pub(super) fn defined_slots(key: &str, value: &Value) -> Vec<SettingSlot> {
    match key {
        SCHEMA_DIRECTIVE_KEY => Vec::new(),
        "catalog" => vec![SettingSlot::Catalog(DEFAULT_CATALOG.to_string())],
        "catalogs" => match value {
            Value::Mapping(catalogs) => catalogs
                .keys()
                .filter_map(Value::as_str)
                .map(|name| SettingSlot::Catalog(name.to_string()))
                .collect(),
            _ => Vec::new(),
        },
        _ => vec![SettingSlot::Setting(canonical_setting_name(key).to_string())],
    }
}

/// Parse the root manifest's include index.
pub(super) fn parse_index(index: &Value, base_dir: &Path) -> Result<Vec<Include>, IncludeError> {
    let Value::Mapping(index) = index else {
        return Err(IncludeError::IndexNotAMapping);
    };
    index
        .iter()
        .map(|(source, delegation)| {
            let source = source.as_str().ok_or(IncludeError::IndexNotAMapping)?;
            parse_include(source, delegation, base_dir)
        })
        .collect()
}

fn parse_include(spec: &str, delegation: &Value, base_dir: &Path) -> Result<Include, IncludeError> {
    let source = parse_source(spec, base_dir)?;
    let delegation = parse_delegation(spec, delegation)?;
    match (&source, &delegation) {
        (IncludeSource::Package(name), Delegation::Unrestricted) => {
            Err(IncludeError::UnrestrictedPackage { name: name.clone() })
        }
        (IncludeSource::Package(name), Delegation::Listed(slots)) => {
            reject_forbidden_package_slots(name, slots)?;
            Ok(Include { source, delegation })
        }
        (IncludeSource::Local { .. }, _) => Ok(Include { source, delegation }),
    }
}

fn parse_source(spec: &str, base_dir: &Path) -> Result<IncludeSource, IncludeError> {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return if is_package_name_shaped(spec) {
            Ok(IncludeSource::Package(spec.to_string()))
        } else {
            Err(IncludeError::InvalidSource { spec: spec.to_string() })
        };
    }
    let invalid = |reason| IncludeError::InvalidLocalPath { spec: spec.to_string(), reason };
    if spec.contains(['*', '?', '[', ']', '{', '}']) {
        return Err(invalid("globs are not supported"));
    }
    let path = base_dir.join(spec);
    let is_yaml = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"));
    if !is_yaml {
        return Err(invalid("the file must have a .yaml or .yml extension"));
    }
    Ok(IncludeSource::Local { spec: spec.to_string(), path })
}

/// A spec that can only be a package name: no path, URL, or protocol syntax.
fn is_package_name_shaped(spec: &str) -> bool {
    let unscoped = match spec.strip_prefix('@') {
        Some(scoped) => match scoped.split_once('/') {
            Some((scope, name)) if !scope.is_empty() => name,
            _ => return false,
        },
        None => spec,
    };
    !unscoped.is_empty() && !unscoped.contains(['/', '\\', ':']) && !unscoped.starts_with('.')
}

fn parse_delegation(spec: &str, delegation: &Value) -> Result<Delegation, IncludeError> {
    let invalid = || IncludeError::InvalidDelegation { spec: spec.to_string() };
    match delegation {
        Value::String(value) if value == UNRESTRICTED => Ok(Delegation::Unrestricted),
        Value::Sequence(selectors) => {
            let mut slots = Vec::with_capacity(selectors.len());
            for selector in selectors {
                let selector = selector.as_str().ok_or_else(invalid)?;
                let slot = SettingSlot::parse_selector(selector)
                    .ok_or_else(|| IncludeError::InvalidSelector {
                        spec: spec.to_string(),
                        selector: selector.to_string(),
                    })?;
                if slots.contains(&slot) {
                    return Err(IncludeError::RepeatedSelector {
                        spec: spec.to_string(),
                        selector: selector.to_string(),
                    });
                }
                slots.push(slot);
            }
            Ok(Delegation::Listed(slots))
        }
        _ => Err(invalid()),
    }
}

fn reject_forbidden_package_slots(name: &str, slots: &[SettingSlot]) -> Result<(), IncludeError> {
    let forbidden = slots
        .iter()
        .find_map(|slot| match slot {
            SettingSlot::Setting(setting)
                if SETTINGS_FORBIDDEN_TO_PACKAGES.contains(&setting.as_str()) =>
            {
                Some(setting)
            }
            _ => None,
        });
    match forbidden {
        Some(setting) => Err(IncludeError::ForbiddenPackageSetting {
            name: name.to_string(),
            setting: setting.clone(),
        }),
        None => Ok(()),
    }
}

/// Every package source must name a declared config dependency.
pub(super) fn check_package_sources(
    includes: &[Include],
    merged: &Mapping,
) -> Result<(), IncludeError> {
    let declared = merged.get("configDependencies").and_then(Value::as_mapping);
    for include in includes {
        let IncludeSource::Package(name) = &include.source else {
            continue;
        };
        if !declared.is_some_and(|declared| declared.contains_key(name.as_str())) {
            return Err(IncludeError::UndeclaredConfigDependency { name: name.clone() });
        }
    }
    Ok(())
}
