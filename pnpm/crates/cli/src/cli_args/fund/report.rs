//! The funding report of one project, shaped like `npm fund --json`.

use super::{
    funding::{is_valid_funding, normalize_funding},
    projects::ProjectDependencies,
};
use crate::cli_args::deps_tree::DependencyNode;
use indexmap::IndexMap;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, path::Path};

pub type FundedDependencies = IndexMap<String, FundedPackage>;

#[derive(Debug, Serialize)]
pub struct FundingReport {
    /// How many listed packages declare funding.
    pub length: usize,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<Value>,
    pub dependencies: FundedDependencies,
}

#[derive(Debug, Serialize)]
pub struct FundedPackage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub funding: Value,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub dependencies: FundedDependencies,
}

/// The name, version and `funding` field an installed package's manifest
/// declares, falling back to the lockfile's name and version.
pub struct InstalledPackage {
    pub name: String,
    pub version: Option<String>,
    pub funding: Option<Value>,
}

impl InstalledPackage {
    pub fn read(node: &DependencyNode) -> Self {
        // A package without a readable manifest has no funding to list.
        let manifest =
            safe_read_package_json_from_dir(Path::new(&node.package.path)).ok().flatten();
        let field = |key: &str| {
            manifest
                .as_ref()
                .and_then(|manifest| manifest.get(key))
        };
        InstalledPackage {
            name: field("name")
                .and_then(Value::as_str)
                .unwrap_or(&node.package.name)
                .to_string(),
            version: field("version")
                .and_then(Value::as_str)
                .or_else(|| {
                    Some(node.package.version.as_str()).filter(|version| !version.is_empty())
                })
                .map(ToString::to_string),
            funding: field("funding").cloned(),
        }
    }
}

impl FundingReport {
    pub fn build(project: &ProjectDependencies) -> Self {
        let mut walk = FundingWalk::default();
        let dependencies = walk.collect(&project.dependencies);
        let field = |key: &str| {
            project.manifest
                .as_ref()
                .and_then(|manifest| manifest.get(key))
        };
        let name = field("name")
            .and_then(Value::as_str)
            .map_or_else(
                || {
                    project.dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                },
                ToString::to_string,
            );
        FundingReport {
            length: walk.funded_count,
            name,
            version: field("version").and_then(Value::as_str).map(ToString::to_string),
            funding: field("funding").filter(|funding| is_truthy(funding)).map(normalize_funding),
            dependencies,
        }
    }
}

/// `libnpmfund` lists each `name@version` once, at the shallowest level it
/// is first reached from: a whole level is marked seen before the walk
/// descends into it. A package without funding passes its funded
/// dependencies up to the nearest funded ancestor.
#[derive(Default)]
struct FundingWalk {
    seen: HashSet<(String, Option<String>)>,
    funded_count: usize,
}

impl FundingWalk {
    fn collect(&mut self, nodes: &[DependencyNode]) -> FundedDependencies {
        let level: Vec<(&DependencyNode, InstalledPackage, bool)> = nodes
            .iter()
            .filter(|node| !node.status.is_skipped)
            .map(|node| {
                let package = InstalledPackage::read(node);
                let first_seen = self.seen.insert((package.name.clone(), package.version.clone()));
                (node, package, first_seen)
            })
            .collect();
        let mut funded = FundedDependencies::new();
        let mut trailing = FundedDependencies::new();
        // A repeated package is still descended into: the tree lists the
        // dependencies of a package only under one of its occurrences.
        for (node, package, first_seen) in level {
            let dependencies = self.collect(&node.dependencies);
            match package.funding.filter(|funding| first_seen && is_valid_funding(funding)) {
                Some(funding) => {
                    self.funded_count += 1;
                    let funding = normalize_funding(&funding);
                    let entry = FundedPackage { version: package.version, funding, dependencies };
                    funded.insert(package.name, entry);
                }
                None => trailing.extend(dependencies),
            }
        }
        funded.extend(trailing);
        funded
    }
}

/// Whether JavaScript would treat the manifest value as set.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(text) => !text.is_empty(),
        Value::Number(number) => number
            .as_f64()
            .is_some_and(|number| number != 0.0),
        Value::Bool(true) | Value::Array(_) | Value::Object(_) => true,
    }
}
