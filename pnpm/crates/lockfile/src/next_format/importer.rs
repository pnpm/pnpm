//! The next format's importers. A dependency is its specifier alone when the
//! version follows from it, and `{specifier, version}` otherwise. Where the
//! project's scope does not say which snapshot it links, the dependency
//! adds `with`, and still leaves out a version that follows from the
//! specifier.
//!
//! The version follows from an exact version, from an `npm:` alias of an
//! exact version, from a `link:` or `file:` path, from a `catalog:`
//! specifier through the `catalogs` section, and from a `workspace:`
//! specifier through the injected package's `name@file:<path>` entry in
//! `packages`. A linked workspace dependency writes its `link:` version, so
//! the lockfile says where it lives without the workspace.

use super::{NamedBindings, NextFormatError};
use crate::{
    CatalogSnapshots, ImporterDepVersion, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
    implied_version::{InjectedPaths, implied_version},
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::collections::HashMap;

/// One dependency of an importer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NextImporterDep {
    Specifier(String),
    Named(Box<NextNamedDep>),
    Resolved(Box<NextResolvedDep>),
}

/// A dependency whose version follows from its specifier, with the bindings
/// that name the snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextNamedDep {
    pub specifier: String,
    #[serde(rename = "with")]
    pub peers: NamedBindings,
}

/// The map form of a dependency as read. `version` is absent when it
/// follows from the specifier.
#[derive(Deserialize)]
struct DepWire {
    specifier: String,
    #[serde(default)]
    version: Option<ImporterDepVersion>,
    #[serde(default, rename = "with")]
    peers: Option<NamedBindings>,
}

impl From<DepWire> for NextImporterDep {
    fn from(wire: DepWire) -> Self {
        let DepWire { specifier, version, peers } = wire;
        match (version, peers) {
            (Some(version), peers) => {
                NextImporterDep::Resolved(Box::new(NextResolvedDep { specifier, version, peers }))
            }
            (None, Some(peers)) => {
                NextImporterDep::Named(Box::new(NextNamedDep { specifier, peers }))
            }
            (None, None) => NextImporterDep::Specifier(specifier),
        }
    }
}

/// A string is the specifier and a map one of the other forms, read directly
/// rather than through an untagged enum's buffering.
impl<'de> Deserialize<'de> for NextImporterDep {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct DepVisitor;

        impl<'de> Visitor<'de> for DepVisitor {
            type Value = NextImporterDep;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a specifier, or a map of specifier with version or bindings")
            }

            fn visit_str<Error: de::Error>(self, text: &str) -> Result<NextImporterDep, Error> {
                Ok(NextImporterDep::Specifier(text.to_owned()))
            }

            fn visit_string<Error: de::Error>(
                self,
                text: String,
            ) -> Result<NextImporterDep, Error> {
                Ok(NextImporterDep::Specifier(text))
            }

            fn visit_map<Map: MapAccess<'de>>(
                self,
                map: Map,
            ) -> Result<NextImporterDep, Map::Error> {
                DepWire::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(NextImporterDep::from)
            }
        }

        deserializer.deserialize_any(DepVisitor)
    }
}

/// A dependency whose version is written out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextResolvedDep {
    pub specifier: String,
    pub version: ImporterDepVersion,
    /// The bindings that name the snapshot, where the project's scope does
    /// not select it.
    #[serde(default, rename = "with", skip_serializing_if = "Option::is_none")]
    pub peers: Option<NamedBindings>,
}

pub type NextImporterDeps = HashMap<PkgName, NextImporterDep>;

/// An importer of the next format. The fields mirror [`ProjectSnapshot`]'s.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextImporter {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<NextImporterDeps>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dev_dependencies: Option<NextImporterDeps>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<NextImporterDeps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies_meta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_directory: Option<bool>,
}

impl NextImporter {
    fn groups(&self) -> impl Iterator<Item = &NextImporterDeps> {
        [&self.dependencies, &self.dev_dependencies, &self.optional_dependencies]
            .into_iter()
            .flatten()
    }

    fn groups_mut(&mut self) -> impl Iterator<Item = &mut NextImporterDeps> {
        [&mut self.dependencies, &mut self.dev_dependencies, &mut self.optional_dependencies]
            .into_iter()
            .flatten()
    }

    /// Every dependency whose version is written out, which after
    /// [`Self::resolve_specifiers`] is every dependency.
    pub fn resolved_dependencies(&self) -> impl Iterator<Item = (&PkgName, &NextResolvedDep)> {
        self.groups()
            .flat_map(HashMap::iter)
            .filter_map(|(alias, dep)| match dep {
                NextImporterDep::Resolved(dep) => Some((alias, dep.as_ref())),
                NextImporterDep::Specifier(_) | NextImporterDep::Named(_) => None,
            })
    }

    /// This importer with every version written out.
    pub fn resolve_specifiers(
        mut self,
        id: &str,
        implied: Implied<'_>,
    ) -> Result<Self, NextFormatError> {
        for map in self.groups_mut() {
            resolve_map(id, map, implied)?;
        }
        Ok(self)
    }

    /// This importer with every dependency whose version follows from its
    /// specifier written without the version: the specifier alone, or with
    /// the bindings that name the snapshot.
    #[must_use]
    pub fn collapsed(&self, implied: Implied<'_>) -> NextImporter {
        let mut collapsed = self.clone();
        for map in collapsed.groups_mut() {
            for (alias, dep) in map.iter_mut() {
                collapse(alias, dep, implied);
            }
        }
        collapsed
    }

    /// Today's importer, with `version` giving each dependency's version.
    pub fn project_snapshot(
        &self,
        version: impl Fn(&PkgName, &NextResolvedDep) -> ImporterDepVersion,
    ) -> ProjectSnapshot {
        let map = |deps: &Option<NextImporterDeps>| {
            deps.as_ref().map(|deps| resolved_map(deps, &version))
        };
        ProjectSnapshot {
            specifiers: None,
            dependencies: map(&self.dependencies),
            dev_dependencies: map(&self.dev_dependencies),
            optional_dependencies: map(&self.optional_dependencies),
            dependencies_meta: self.dependencies_meta.clone(),
            publish_directory: self.publish_directory.clone(),
            link_directory: self.link_directory,
        }
    }
}

fn resolved_map(
    deps: &NextImporterDeps,
    version: &impl Fn(&PkgName, &NextResolvedDep) -> ImporterDepVersion,
) -> ResolvedDependencyMap {
    deps.iter()
        .filter_map(|(alias, dep)| match dep {
            NextImporterDep::Resolved(dep) => Some((alias, dep.as_ref())),
            NextImporterDep::Specifier(_) | NextImporterDep::Named(_) => None,
        })
        .map(|(alias, dep)| {
            let spec = ResolvedDependencySpec {
                specifier: dep.specifier.clone(),
                version: version(alias, dep),
            };
            (alias.clone(), spec)
        })
        .collect()
}

/// What a specifier's version follows from: the `catalogs` section and the
/// injected packages' entries.
#[derive(Clone, Copy)]
pub struct Implied<'a> {
    pub catalogs: Option<&'a CatalogSnapshots>,
    pub injected: &'a InjectedPaths,
}

impl Implied<'_> {
    /// The version `specifier` implies for `alias`, if any.
    fn version(self, alias: &PkgName, specifier: &str) -> Option<String> {
        implied_version(
            &alias.to_string(),
            specifier,
            |catalog, alias| {
                let entry = self.catalogs?.get(catalog)?.get(alias)?;
                Some((entry.specifier.clone(), entry.version.clone()))
            },
            |name| self.injected.get(name).map(String::as_str),
        )
    }
}

fn resolve_map(
    id: &str,
    map: &mut NextImporterDeps,
    implied: Implied<'_>,
) -> Result<(), NextFormatError> {
    for (alias, dep) in map.iter_mut() {
        let (specifier, peers) = match dep {
            NextImporterDep::Specifier(specifier) => (std::mem::take(specifier), None),
            NextImporterDep::Named(named) => {
                (std::mem::take(&mut named.specifier), Some(std::mem::take(&mut named.peers)))
            }
            NextImporterDep::Resolved(_) => continue,
        };
        let mut resolved = resolved(id, alias, specifier, implied)?;
        resolved.peers = peers;
        *dep = NextImporterDep::Resolved(Box::new(resolved));
    }
    Ok(())
}

fn resolved(
    id: &str,
    alias: &PkgName,
    specifier: String,
    implied: Implied<'_>,
) -> Result<NextResolvedDep, NextFormatError> {
    let underivable = || NextFormatError::UnderivableVersion {
        importer: id.to_string(),
        name: alias.to_string(),
        specifier: specifier.clone(),
    };
    let version = implied
        .version(alias, &specifier)
        .ok_or_else(underivable)?
        .parse::<ImporterDepVersion>()
        .map_err(|_| underivable())?;
    Ok(NextResolvedDep { specifier, version, peers: None })
}

fn collapse(alias: &PkgName, dep: &mut NextImporterDep, implied: Implied<'_>) {
    let NextImporterDep::Resolved(resolved) = dep else { return };
    let implied = implied.version(alias, &resolved.specifier);
    if implied.is_none_or(|implied| implied != resolved.version.to_string()) {
        return;
    }
    let specifier = std::mem::take(&mut resolved.specifier);
    *dep = match resolved.peers.take() {
        Some(peers) => NextImporterDep::Named(Box::new(NextNamedDep { specifier, peers })),
        None => NextImporterDep::Specifier(specifier),
    };
}
