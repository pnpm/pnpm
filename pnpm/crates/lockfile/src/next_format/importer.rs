//! The next format's importers. A dependency is its specifier alone when the
//! version follows from it, and `{specifier, version}` otherwise.
//!
//! The version follows from an exact version, from an `npm:` alias of an
//! exact version, from a `link:` or `file:` path, and from a `catalog:`
//! specifier through the `catalogs` section. A `workspace:` specifier
//! always writes its `link:` version, so the lockfile says where the
//! workspace dependency lives without the workspace.

use super::NextFormatError;
use crate::{
    CatalogSnapshots, ImporterDepVersion, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// One dependency of an importer, as read.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum NextImporterDep {
    Specifier(String),
    Resolved(ResolvedDependencySpec),
}

/// An importer as read. The fields mirror [`ProjectSnapshot`]'s.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextImporter {
    #[serde(default)]
    pub dependencies: Option<HashMap<PkgName, NextImporterDep>>,
    #[serde(default)]
    pub dev_dependencies: Option<HashMap<PkgName, NextImporterDep>>,
    #[serde(default)]
    pub optional_dependencies: Option<HashMap<PkgName, NextImporterDep>>,
    #[serde(default)]
    pub dependencies_meta: Option<serde_json::Value>,
    #[serde(default)]
    pub publish_directory: Option<String>,
    #[serde(default)]
    pub link_directory: Option<bool>,
}

impl NextImporter {
    /// The importer with every version written out.
    pub fn into_project_snapshot(
        self,
        id: &str,
        catalogs: Option<&CatalogSnapshots>,
    ) -> Result<ProjectSnapshot, NextFormatError> {
        Ok(ProjectSnapshot {
            specifiers: None,
            dependencies: resolved_map(id, self.dependencies, catalogs)?,
            dev_dependencies: resolved_map(id, self.dev_dependencies, catalogs)?,
            optional_dependencies: resolved_map(id, self.optional_dependencies, catalogs)?,
            dependencies_meta: self.dependencies_meta,
            publish_directory: self.publish_directory,
            link_directory: self.link_directory,
        })
    }
}

fn resolved_map(
    id: &str,
    map: Option<HashMap<PkgName, NextImporterDep>>,
    catalogs: Option<&CatalogSnapshots>,
) -> Result<Option<ResolvedDependencyMap>, NextFormatError> {
    let Some(map) = map else { return Ok(None) };
    let mut resolved_entries = ResolvedDependencyMap::with_capacity(map.len());
    for (alias, dep) in map {
        let spec = resolved(id, &alias, dep, catalogs)?;
        resolved_entries.insert(alias, spec);
    }
    Ok(Some(resolved_entries))
}

fn resolved(
    id: &str,
    alias: &PkgName,
    dep: NextImporterDep,
    catalogs: Option<&CatalogSnapshots>,
) -> Result<ResolvedDependencySpec, NextFormatError> {
    let specifier = match dep {
        NextImporterDep::Resolved(spec) => return Ok(spec),
        NextImporterDep::Specifier(specifier) => specifier,
    };
    let underivable = || NextFormatError::UnderivableVersion {
        importer: id.to_string(),
        name: alias.to_string(),
        specifier: specifier.clone(),
    };
    let version = implied_version(&alias.to_string(), &specifier, catalog_entry(catalogs))
        .ok_or_else(underivable)?
        .parse::<ImporterDepVersion>()
        .map_err(|_| underivable())?;
    Ok(ResolvedDependencySpec { specifier, version })
}

/// The version `specifier` implies for `alias`, or `None` when it implies
/// none, as for a range. `catalog_entry(catalog, alias)` is the specifier
/// and version the `catalogs` section records for a `catalog:` specifier.
pub(crate) fn implied_version(
    alias: &str,
    specifier: &str,
    catalog_entry: impl FnOnce(&str, &str) -> Option<(String, String)>,
) -> Option<String> {
    if let Some(catalog) = specifier.strip_prefix("catalog:") {
        let catalog = if catalog.is_empty() { "default" } else { catalog };
        let (catalog_specifier, version) = catalog_entry(catalog, alias)?;
        return Some(match catalog_specifier.strip_prefix("npm:") {
            Some(target) => format!("{}@{version}", alias_name(target)),
            None => version,
        });
    }
    Some(
        specifier
            .strip_prefix("npm:")
            .unwrap_or(specifier)
            .to_string(),
    )
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

/// The package name of an `npm:` alias target, `name` or `name@range`.
fn alias_name(target: &str) -> &str {
    match target[1..].find('@') {
        Some(at) => &target[..=at],
        None => target,
    }
}

/// [`NextImporter`] borrowed from the importer it is written from.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextImporterOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    dependencies: Option<BTreeMap<String, NextImporterDepOut<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev_dependencies: Option<BTreeMap<String, NextImporterDepOut<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_dependencies: Option<BTreeMap<String, NextImporterDepOut<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependencies_meta: &'a Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publish_directory: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_directory: &'a Option<bool>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum NextImporterDepOut<'a> {
    Specifier(&'a str),
    Resolved(&'a ResolvedDependencySpec),
}

impl<'a> NextImporterOut<'a> {
    #[must_use]
    pub fn new(importer: &'a ProjectSnapshot, catalogs: Option<&CatalogSnapshots>) -> Self {
        let out = |map: &'a Option<ResolvedDependencyMap>| {
            map.as_ref()
                .map(|map| {
                    map.iter()
                        .map(|(alias, spec)| (alias.to_string(), dep_out(alias, spec, catalogs)))
                        .collect()
                })
        };
        NextImporterOut {
            dependencies: out(&importer.dependencies),
            dev_dependencies: out(&importer.dev_dependencies),
            optional_dependencies: out(&importer.optional_dependencies),
            dependencies_meta: &importer.dependencies_meta,
            publish_directory: &importer.publish_directory,
            link_directory: &importer.link_directory,
        }
    }
}

fn dep_out<'a>(
    alias: &PkgName,
    spec: &'a ResolvedDependencySpec,
    catalogs: Option<&CatalogSnapshots>,
) -> NextImporterDepOut<'a> {
    let implied = implied_version(&alias.to_string(), &spec.specifier, catalog_entry(catalogs));
    if implied.is_some_and(|implied| implied == spec.version.to_string()) {
        NextImporterDepOut::Specifier(&spec.specifier)
    } else {
        NextImporterDepOut::Resolved(spec)
    }
}
