//! The next format's importers. A dependency is its specifier alone when the
//! version follows from it, and `{specifier, version}` otherwise, with
//! `with` where the project's scope does not say which snapshot it links.
//!
//! The version follows from an exact version, from an `npm:` alias of an
//! exact version, from a `link:` or `file:` path, and from a `catalog:`
//! specifier through the `catalogs` section. A `workspace:` specifier
//! always writes its `link:` version, so the lockfile says where the
//! workspace dependency lives without the workspace.

use super::{NamedBindings, NextFormatError};
use crate::{
    CatalogSnapshots, ImporterDepVersion, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, implied_version::implied_version,
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
    Resolved(Box<NextResolvedDep>),
}

/// A string is the specifier and a map the resolved form, read directly
/// rather than through an untagged enum's buffering.
impl<'de> Deserialize<'de> for NextImporterDep {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct DepVisitor;

        impl<'de> Visitor<'de> for DepVisitor {
            type Value = NextImporterDep;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a specifier, or a map of specifier and version")
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
                NextResolvedDep::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|dep| NextImporterDep::Resolved(Box::new(dep)))
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
                NextImporterDep::Specifier(_) => None,
            })
    }

    /// This importer with every version written out.
    pub fn resolve_specifiers(
        mut self,
        id: &str,
        catalogs: Option<&CatalogSnapshots>,
    ) -> Result<Self, NextFormatError> {
        for map in self.groups_mut() {
            resolve_map(id, map, catalogs)?;
        }
        Ok(self)
    }

    /// This importer with every dependency whose version follows from its
    /// specifier, and which names no snapshot, written as the specifier.
    #[must_use]
    pub fn collapsed(&self, catalogs: Option<&CatalogSnapshots>) -> NextImporter {
        let mut collapsed = self.clone();
        for map in collapsed.groups_mut() {
            for (alias, dep) in map.iter_mut() {
                collapse(alias, dep, catalogs);
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
            NextImporterDep::Specifier(_) => None,
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

fn resolve_map(
    id: &str,
    map: &mut NextImporterDeps,
    catalogs: Option<&CatalogSnapshots>,
) -> Result<(), NextFormatError> {
    for (alias, dep) in map.iter_mut() {
        if let NextImporterDep::Specifier(specifier) = dep {
            let specifier = std::mem::take(specifier);
            *dep = NextImporterDep::Resolved(Box::new(resolved(id, alias, specifier, catalogs)?));
        }
    }
    Ok(())
}

fn resolved(
    id: &str,
    alias: &PkgName,
    specifier: String,
    catalogs: Option<&CatalogSnapshots>,
) -> Result<NextResolvedDep, NextFormatError> {
    let underivable = || NextFormatError::UnderivableVersion {
        importer: id.to_string(),
        name: alias.to_string(),
        specifier: specifier.clone(),
    };
    let version = implied_version(&alias.to_string(), &specifier, catalog_entry(catalogs))
        .ok_or_else(underivable)?
        .parse::<ImporterDepVersion>()
        .map_err(|_| underivable())?;
    Ok(NextResolvedDep { specifier, version, peers: None })
}

fn collapse(alias: &PkgName, dep: &mut NextImporterDep, catalogs: Option<&CatalogSnapshots>) {
    let NextImporterDep::Resolved(resolved) = dep else { return };
    if resolved.peers.is_some() {
        return;
    }
    let implied = implied_version(&alias.to_string(), &resolved.specifier, catalog_entry(catalogs));
    if implied.is_some_and(|implied| implied == resolved.version.to_string()) {
        *dep = NextImporterDep::Specifier(std::mem::take(&mut resolved.specifier));
    }
}

/// The lookup [`implied_version`] takes, over the typed `catalogs` section.
fn catalog_entry(
    catalogs: Option<&CatalogSnapshots>,
) -> impl FnOnce(&str, &str) -> Option<(String, String)> {
    move |catalog, alias| {
        let entry = catalogs?.get(catalog)?.get(alias)?;
        Some((entry.specifier.clone(), entry.version.clone()))
    }
}
