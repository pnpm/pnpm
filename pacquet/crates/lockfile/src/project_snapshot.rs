use crate::{PkgName, ResolvedDependencyMap, ResolvedDependencySpec};
use pacquet_package_manifest::DependencyGroup;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Snapshot of a single project.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    /// Direct-dependency specifiers, keyed by alias. The v9 lockfile file
    /// format does not carry a top-level `specifiers` map — each specifier is
    /// inlined next to its resolved version in the dependency blocks (see
    /// [`ResolvedDependencySpec`]) — so this field is never serialized. pnpm's
    /// in-memory `ProjectSnapshot` keeps it for catalog-snapshot construction,
    /// and pacquet does the same; it also still deserializes from older
    /// lockfiles that recorded it.
    #[serde(default, skip_serializing)]
    pub specifiers: Option<HashMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<ResolvedDependencyMap>,
    // Field order mirrors the v9 importer block pnpm writes: `dependencies`,
    // then `devDependencies`, then `optionalDependencies`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dev_dependencies: Option<ResolvedDependencyMap>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<ResolvedDependencyMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies_meta: Option<serde_json::Value>, // TODO: DependenciesMeta
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_directory: Option<String>,
}

impl ProjectSnapshot {
    /// Lookup dependency map according to group.
    #[must_use]
    pub fn get_map_by_group(&self, group: DependencyGroup) -> Option<&'_ ResolvedDependencyMap> {
        match group {
            DependencyGroup::Prod => self.dependencies.as_ref(),
            DependencyGroup::Optional => self.optional_dependencies.as_ref(),
            DependencyGroup::Dev => self.dev_dependencies.as_ref(),
            DependencyGroup::Peer => None,
        }
    }

    /// Iterate over combination of dependency maps according to groups.
    pub fn dependencies_by_groups(
        &self,
        groups: impl IntoIterator<Item = DependencyGroup>,
    ) -> impl Iterator<Item = (&'_ PkgName, &'_ ResolvedDependencySpec)> {
        groups.into_iter().filter_map(|group| self.get_map_by_group(group)).flatten()
    }
}

#[cfg(test)]
mod tests;
