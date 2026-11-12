//! The edits the admin API makes to a [`RegistryGraphDocument`]. Each edit
//! changes the document only; [`RoutingConfig::from_document`](crate::RoutingConfig::from_document)
//! validates the result.

use indexmap::IndexMap;
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;
use serde::{Deserialize, Serialize};

use super::{RegistryFile, RegistryGraphDocument};
use crate::{AccessSpec, PackageAccess, access::TeamFile, config_file::HostedFile};

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

    /// The `url` of the upstream registry `name`, as the document writes it.
    #[must_use]
    pub fn upstream_url(&self, name: &str) -> Option<&str> {
        match self.registries.get(name)? {
            RegistryFile::Upstream(upstream) => Some(&upstream.url),
            RegistryFile::Hosted(_) | RegistryFile::Router(_) => None,
        }
    }

    /// The ID of the team named `team` on the hosted registry `registry`.
    #[must_use]
    pub fn team_id(&self, registry: &str, team: &str) -> Option<&str> {
        let RegistryFile::Hosted(hosted) = self.registries.get(registry)? else {
            return None;
        };
        hosted.teams
            .iter()
            .find(|(_, declared)| declared.name == team)
            .map(|(id, _)| id.as_str())
    }

    /// Add an empty team to the hosted registry `registry`.
    pub fn create_team(
        &mut self,
        registry: &str,
        id: &str,
        team: &str,
    ) -> Result<(), RegistryError> {
        if self.team_id(registry, team).is_some() {
            return Err(RegistryError::AdminConflict {
                reason: format!("team {team:?} already exists"),
            });
        }
        let new_team = TeamFile { name: team.to_string(), members: AccessSpec::Many(Vec::new()) };
        self.hosted_mut(registry)?.teams.insert(id.to_string(), new_team);
        Ok(())
    }

    /// Remove the team named `team`, returning its ID. A team the registry's
    /// access lists name is refused, so no list loses the team it grants to.
    pub fn remove_team(&mut self, registry: &str, team: &str) -> Result<String, RegistryError> {
        let id = self
            .team_id(registry, team)
            .ok_or(RegistryError::NotFound)?
            .to_string();
        let hosted = self.hosted_mut(registry)?;
        if names_team(hosted, &id) {
            return Err(RegistryError::AdminConflict {
                reason: format!(
                    "team {team:?} is named by this registry's access rules; remove it from them first",
                ),
            });
        }
        hosted.teams.shift_remove(&id);
        Ok(id)
    }

    /// Add `user_id` to the team named `team`. Adding a member twice is a
    /// no-op.
    pub fn add_team_member(
        &mut self,
        registry: &str,
        team: &str,
        user_id: &str,
    ) -> Result<(), RegistryError> {
        self.edit_team_members(registry, team, |members| {
            if !members
                .iter()
                .any(|member| member == user_id)
            {
                members.push(user_id.to_string());
            }
        })
    }

    /// Remove `user_id` from the team named `team`. Removing a non-member is a
    /// no-op.
    pub fn remove_team_member(
        &mut self,
        registry: &str,
        team: &str,
        user_id: &str,
    ) -> Result<(), RegistryError> {
        self.edit_team_members(registry, team, |members| members.retain(|member| member != user_id))
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

    fn edit_team_members(
        &mut self,
        registry: &str,
        team: &str,
        edit: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), RegistryError> {
        let id = self
            .team_id(registry, team)
            .ok_or(RegistryError::NotFound)?
            .to_string();
        let declared = self
            .hosted_mut(registry)?
            .teams
            .get_mut(&id)
            .ok_or(RegistryError::NotFound)?;
        let mut members = declared.members.entries().to_vec();
        edit(&mut members);
        declared.members = AccessSpec::Many(members);
        Ok(())
    }
}

/// Whether any access list of `hosted` names the team `id`.
fn names_team(hosted: &HostedFile, id: &str) -> bool {
    let token = format!("team:{id}");
    let names =
        |spec: Option<&AccessSpec>| spec.is_some_and(|spec| spec.entries().contains(&token));
    names(hosted.access.as_ref())
        || hosted.packages
            .values()
            .flatten()
            .any(|rule| {
                names(rule.access.as_ref())
                    || names(rule.publish.as_ref())
                    || names(rule.unpublish.as_ref())
            })
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
