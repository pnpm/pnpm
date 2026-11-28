//! The edits the admin API makes to a [`RegistryGraphDocument`]. Each edit
//! changes the document only; [`RoutingConfig::from_document`](crate::RoutingConfig::from_document)
//! validates the result.

use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;
use serde::Serialize;

use super::{
    RegistryFile,
    RegistryGraphDocument,
};

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

/// What a registry of the graph is, as far as its default grant goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryKind {
    Hosted,
    /// An upstream fetched anonymously.
    PublicUpstream,
    /// An upstream pnpr sends a credential to.
    PrivateUpstream,
    Router,
}

impl RegistryGraphDocument {
    #[must_use]
    pub fn registry_kind(&self, name: &str) -> Option<RegistryKind> {
        Some(match self.registries.get(name)? {
            RegistryFile::Hosted(_) => RegistryKind::Hosted,
            RegistryFile::Upstream(upstream) if upstream.public => RegistryKind::PublicUpstream,
            RegistryFile::Upstream(_) => RegistryKind::PrivateUpstream,
            RegistryFile::Router(_) => RegistryKind::Router,
        })
    }

    pub fn registry_names(&self) -> impl Iterator<Item = &str> {
        self.registries.keys().map(String::as_str)
    }

    /// One `registries:` entry in its JSON shape.
    #[must_use]
    pub fn registry_entry(&self, name: &str) -> Option<serde_json::Value> {
        self.registries
            .get(name)
            .map(serde_json::to_value)
            .and_then(Result::ok)
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
            RegistryFile::Upstream(upstream) => upstream.origin_url(),
            RegistryFile::Hosted(_) | RegistryFile::Router(_) => None,
        }
    }
}
