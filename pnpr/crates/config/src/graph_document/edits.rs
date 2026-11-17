//! The edits the admin API makes to a [`RegistryGraphDocument`]. Each edit
//! changes the document only; [`RoutingConfig::from_document`](crate::RoutingConfig::from_document)
//! validates the result.

use indexmap::IndexMap;
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::{RegistryFile, RegistryGraphDocument};
use crate::{AccessSpec, PackageAccess, config_file::HostedFile};

/// The registries a document change adds, removes, and changes, by name.
/// `changed` also names `defaultRegistry` and `routes` when they change.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct RegistryGraphChanges {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl RegistryGraphChanges {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Where a hosted registry keeps its packages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedStorage {
    /// The storage namespace; `""` is the default namespace.
    pub namespace: String,
    pub ecosystem: Ecosystem,
}

/// The access lists of a hosted registry: its registry-level `access` and the
/// lists of each `packages:` key. `None` is an omitted list, which falls back
/// to the registry default.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleLists {
    pub access: Option<Vec<String>>,
    pub packages: IndexMap<String, PackageLists>,
}

/// The lists of one `packages:` key.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageLists {
    pub access: Option<Vec<String>>,
    pub publish: Option<Vec<String>>,
    pub unpublish: Option<Vec<String>>,
}

impl RegistryGraphDocument {
    pub fn registry_names(&self) -> impl Iterator<Item = &str> {
        self.registries.keys().map(String::as_str)
    }

    /// One `registries:` entry in its JSON shape.
    #[must_use]
    pub fn registry_entry(&self, name: &str) -> Option<serde_json::Value> {
        self.registries
            .get(name)
            .and_then(|entry| serde_json::to_value(entry).ok())
    }

    /// Add `entry` under `name`, or replace the entry already there in place.
    pub fn set_registry_entry(
        &mut self,
        name: &str,
        entry: serde_json::Value,
    ) -> Result<(), RegistryError> {
        let entry = serde_json::from_value::<RegistryFile>(entry)
            .map_err(|err| RegistryError::BadRequest {
                reason: format!("registry {name:?}: {err}"),
            })?;
        self.registries.insert(name.to_string(), entry);
        Ok(())
    }

    /// Remove the entry under `name`, keeping the order of the rest. Reports
    /// whether there was one.
    pub fn remove_registry(&mut self, name: &str) -> bool {
        self.registries.shift_remove(name).is_some()
    }

    /// What changed from `previous` to this document.
    #[must_use]
    pub fn changes_since(&self, previous: &Self) -> RegistryGraphChanges {
        let shape = |entry: &RegistryFile| serde_json::to_value(entry).ok();
        let mut changes = RegistryGraphChanges::default();
        for (name, entry) in &self.registries {
            match previous.registries.get(name) {
                None => changes.added.push(name.clone()),
                Some(before) if shape(before) != shape(entry) => changes.changed.push(name.clone()),
                Some(_) => {}
            }
        }
        changes.removed = previous.registries
            .keys()
            .filter(|name| !self.registries.contains_key(*name))
            .cloned()
            .collect();
        let default_shape = |document: &Self| serde_json::to_value(&document.default_registry).ok();
        if default_shape(self) != default_shape(previous) {
            changes.changed.push("defaultRegistry".to_string());
        }
        let routes_shape = |document: &Self| serde_json::to_value(&document.routes).ok();
        if routes_shape(self) != routes_shape(previous) {
            changes.changed.push("routes".to_string());
        }
        changes
    }

    /// The namespace and ecosystem of the hosted registry `name`.
    #[must_use]
    pub fn hosted_storage(&self, name: &str) -> Option<HostedStorage> {
        match self.registries.get(name)? {
            RegistryFile::Hosted(hosted) => Some(HostedStorage {
                namespace: hosted.namespace.clone().unwrap_or_default(),
                ecosystem: hosted.ecosystem.unwrap_or_default(),
            }),
            RegistryFile::Upstream(_) | RegistryFile::Router(_) => None,
        }
    }

    /// The `registry` of each `routes.public` entry that names one.
    pub fn public_route_registries(&self) -> impl Iterator<Item = &str> {
        self.routes.public.iter().filter_map(|route| route.registry.as_deref())
    }

    /// The `url` of the upstream registry `name`, as the document writes it.
    #[must_use]
    pub fn upstream_url(&self, name: &str) -> Option<&str> {
        match self.registries.get(name)? {
            RegistryFile::Upstream(upstream) => Some(&upstream.url),
            RegistryFile::Hosted(_) | RegistryFile::Router(_) => None,
        }
    }

    /// The IDs of the teams the access lists of every registry name.
    #[must_use]
    pub fn named_teams(&self) -> BTreeSet<&str> {
        self.registries
            .values()
            .flat_map(|registry| {
                let (access, packages) = match registry {
                    RegistryFile::Hosted(hosted) => {
                        (hosted.access.as_ref(), Some(&hosted.packages))
                    }
                    RegistryFile::Upstream(upstream) => {
                        (upstream.access.as_ref(), Some(&upstream.packages))
                    }
                    RegistryFile::Router(_) => (None, None),
                };
                let rules = packages
                    .into_iter()
                    .flat_map(IndexMap::values)
                    .flatten()
                    .flat_map(|rule| [&rule.access, &rule.publish, &rule.unpublish])
                    .flatten();
                access
                    .into_iter()
                    .chain(rules)
                    .flat_map(AccessSpec::teams)
            })
            .collect()
    }

    /// The access lists of the hosted registry `registry`.
    #[must_use]
    pub fn rule_lists(&self, registry: &str) -> Option<RuleLists> {
        let RegistryFile::Hosted(hosted) = self.registries.get(registry)? else {
            return None;
        };
        let packages = hosted.packages
            .iter()
            .map(|(key, rule)| (key.clone(), package_lists(rule.as_ref())))
            .collect();
        Some(RuleLists { access: list(hosted.access.as_ref()), packages })
    }

    /// Set the hosted registry's `access` to `lists.access`, and the lists of
    /// each key in `lists.packages`. Keys `lists.packages` leaves out keep their
    /// lists. A key the registry's `packages:` map does not declare is refused,
    /// because the keys decide which registry serves a name.
    pub fn set_rule_lists(
        &mut self,
        registry: &str,
        lists: RuleLists,
    ) -> Result<(), RegistryError> {
        let hosted = self.hosted_mut(registry)?;
        if let Some(key) = lists.packages
            .keys()
            .find(|key| !hosted.packages.contains_key(*key))
        {
            return Err(RegistryError::BadRequest {
                reason: format!("registry {registry:?} does not declare the pattern {key:?}"),
            });
        }
        hosted.access = lists.access.map(AccessSpec::Many);
        for (key, package) in lists.packages {
            hosted.packages.insert(key, package_access(package));
        }
        Ok(())
    }

    fn hosted_mut(&mut self, registry: &str) -> Result<&mut HostedFile, RegistryError> {
        match self.registries.get_mut(registry) {
            Some(RegistryFile::Hosted(hosted)) => Ok(hosted),
            _ => Err(RegistryError::NotFound),
        }
    }
}

fn list(spec: Option<&AccessSpec>) -> Option<Vec<String>> {
    spec.map(|spec| spec.entries().to_vec())
}

fn package_lists(rule: Option<&PackageAccess>) -> PackageLists {
    PackageLists {
        access: list(rule.and_then(|rule| rule.access.as_ref())),
        publish: list(rule.and_then(|rule| rule.publish.as_ref())),
        unpublish: list(rule.and_then(|rule| rule.unpublish.as_ref())),
    }
}

fn package_access(lists: PackageLists) -> Option<PackageAccess> {
    if lists.access.is_none() && lists.publish.is_none() && lists.unpublish.is_none() {
        return None;
    }
    Some(PackageAccess {
        access: lists.access.map(AccessSpec::Many),
        publish: lists.publish.map(AccessSpec::Many),
        unpublish: lists.unpublish.map(AccessSpec::Many),
    })
}
