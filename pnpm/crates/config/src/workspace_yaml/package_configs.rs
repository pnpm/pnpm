//! `packageConfigs`: the settings a workspace sets for one project
//! instead of for all of them.
//!
//! The setting is written either as a map from project name to its
//! settings, or as a list of entries that each name the projects they
//! apply to. Both forms flatten to the same `project name → settings`
//! lookup, which [`crate::Config::anchor_dedicated_project`] overlays
//! onto the config of the project it names.

use crate::Config;
use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, SeqAccess, Visitor, value::SeqAccessDeserializer},
};
use std::{fmt, path::Path};

/// The settings one `packageConfigs` entry may set.
///
/// Every field names a setting `pnpm-workspace.yaml` also carries at
/// its top level; the entry replaces the value the project would
/// otherwise get, whichever layer supplied it. A per-project entry
/// outranking an environment variable or a command-line flag is what
/// pnpm 11 does, and the setting has no other way to say "this project
/// is the exception".
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ProjectConfig {
    /// `hoist`. `false` also clears [`Config::hoist_pattern`], the way
    /// a top-level `hoist: false` does. `true` sets only the flag: the
    /// pattern a workspace-wide `hoist: false` already cleared is gone,
    /// so a project cannot opt back into hoisting a workspace turned
    /// off — as in pnpm 11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    /// `modulesDir`, resolved against the project's own directory the
    /// way the workspace-wide setting resolves against the workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules_dir: Option<String>,
    /// `overrides`. Replaces the workspace-wide map rather than
    /// merging into it, so the project resolves against exactly the
    /// selectors listed here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<IndexMap<String, String>>,
    /// `saveExact`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_exact: Option<bool>,
    /// `savePrefix`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_prefix: Option<String>,
}

impl ProjectConfig {
    /// Overlay the settings onto `config`, resolving a relative
    /// `modulesDir` against `project_dir`.
    ///
    /// Applied after [`Config::anchor_lockfile_paths`] has anchored the
    /// project's paths, so a `modulesDir` here outranks the one the
    /// workspace set for everyone.
    pub(crate) fn apply_to(self, config: &mut Config, project_dir: &Path) {
        if let Some(hoist) = self.hoist {
            config.hoist = hoist;
            if !hoist {
                config.hoist_pattern = None;
            }
        }
        if let Some(modules_dir) = self.modules_dir {
            // The same resolution the top-level `modulesDir` gets, so a
            // project entry and the workspace-wide setting read a value
            // the same way.
            config.modules_dir = super::resolve(project_dir, &modules_dir);
            // The same derivation `anchor_lockfile_paths` runs: the
            // virtual store follows the modules dir unless the
            // workspace pinned it, and a global virtual store is
            // store-anchored and follows nothing.
            if !config.enable_global_virtual_store
                && !config.explicit_settings.contains_key("virtualStoreDir")
            {
                config.virtual_store_dir = config.modules_dir.join(".pnpm");
            }
        }
        if let Some(overrides) = self.overrides {
            config.overrides = (!overrides.is_empty()).then_some(overrides);
        }
        if let Some(save_exact) = self.save_exact {
            config.save_exact = save_exact;
        }
        if let Some(save_prefix) = self.save_prefix {
            config.save_prefix = Some(save_prefix);
        }
    }
}

/// One entry of the list form of `packageConfigs`: the settings, plus
/// the project names they apply to.
///
/// The settings are spelled out rather than flattened in from
/// [`ProjectConfig`] because serde refuses `deny_unknown_fields` on a
/// struct that flattens another, and reporting a misspelled setting is
/// the point of that attribute here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfigMultiMatch {
    /// The names of the projects the entry applies to.
    pub r#match: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_exact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_prefix: Option<String>,
}

impl From<ProjectConfigMultiMatch> for ProjectConfig {
    fn from(entry: ProjectConfigMultiMatch) -> Self {
        let ProjectConfigMultiMatch {
            r#match: _,
            hoist,
            modules_dir,
            overrides,
            save_exact,
            save_prefix,
        } = entry;
        ProjectConfig { hoist, modules_dir, overrides, save_exact, save_prefix }
    }
}

/// The `packageConfigs` setting as written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PackageConfigsSetting {
    /// `packageConfigs: { <project name>: { ... } }`.
    ByName(IndexMap<String, ProjectConfig>),
    /// `packageConfigs: [{ match: [<project name>], ... }]`.
    Matches(Vec<ProjectConfigMultiMatch>),
}

impl PackageConfigsSetting {
    /// Flatten to the `project name → settings` lookup the install
    /// reads. A later entry wins over an earlier one naming the same
    /// project.
    #[must_use]
    pub fn into_record(self) -> IndexMap<String, ProjectConfig> {
        match self {
            Self::ByName(record) => record,
            Self::Matches(entries) => entries
                .into_iter()
                .flat_map(|mut entry| {
                    let names = std::mem::take(&mut entry.r#match);
                    let config = ProjectConfig::from(entry);
                    names.into_iter().map(move |name| (name, config.clone()))
                })
                .collect(),
        }
    }
}

impl<'de> Deserialize<'de> for PackageConfigsSetting {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        deserializer.deserialize_any(PackageConfigsSettingVisitor)
    }
}

/// Written out rather than derived as `#[serde(untagged)]` so that the
/// error from a malformed entry survives: an untagged enum reports only
/// that no variant matched, which would hide, say, a `saveExact` that is
/// not a boolean.
struct PackageConfigsSettingVisitor;

impl<'de> Visitor<'de> for PackageConfigsSettingVisitor {
    type Value = PackageConfigsSetting;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("packageConfigs to be either an object or an array")
    }

    fn visit_map<Map: MapAccess<'de>>(self, mut map: Map) -> Result<Self::Value, Map::Error> {
        let mut record = IndexMap::new();
        while let Some((name, config)) = map.next_entry::<String, ProjectConfig>()? {
            record.insert(name, config);
        }
        Ok(PackageConfigsSetting::ByName(record))
    }

    fn visit_seq<Seq: SeqAccess<'de>>(self, seq: Seq) -> Result<Self::Value, Seq::Error> {
        Vec::deserialize(SeqAccessDeserializer::new(seq)).map(PackageConfigsSetting::Matches)
    }
}

#[cfg(test)]
mod tests;
