use crate::{PkgName, ResolvedDependencyMap, ResolvedDependencySpec};
use pacquet_package_manifest::DependencyGroup;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Snapshot of a single project.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub specifiers: Option<HashMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<ResolvedDependencyMap>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<ResolvedDependencyMap>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dev_dependencies: Option<ResolvedDependencyMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies_meta: Option<serde_json::Value>, // TODO: DependenciesMeta
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_directory: Option<String>,
}

impl ProjectSnapshot {
    /// Lookup dependency map according to group.
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
        groups.into_iter().flat_map(|group| self.get_map_by_group(group)).flatten()
    }
}

#[cfg(test)]
mod tests;
