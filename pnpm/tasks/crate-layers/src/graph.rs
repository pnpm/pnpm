use crate::tree::Tree;
use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DependencyKind {
    Normal,
    Build,
    Dev,
}

impl DependencyKind {
    const TABLES: [(&'static str, Self); 3] = [
        ("dependencies", Self::Normal),
        ("build-dependencies", Self::Build),
        ("dev-dependencies", Self::Dev),
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Build => "build",
            Self::Dev => "dev",
        }
    }

    /// Whether the dependency ends up in the crate's own build, which is what
    /// the layer rule constrains. Tests may reach any layer.
    pub const fn is_linked(self) -> bool {
        !matches!(self, Self::Dev)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
}

/// The workspace's crates and the dependencies between them. Dependencies on
/// crates outside the workspace are left out.
#[derive(Debug, Default)]
pub struct CrateGraph {
    pub crates: BTreeSet<String>,
    pub edges: BTreeSet<Edge>,
    /// Each crate's directory, relative to the repository root.
    pub dirs: BTreeMap<String, String>,
}

type Manifest = (String, Table);

pub fn read(tree: &dyn Tree) -> Result<CrateGraph, String> {
    let root = parse(tree, "Cargo.toml")?.ok_or("the tree has no Cargo.toml")?;
    let workspace = root
        .get("workspace")
        .and_then(Value::as_table)
        .ok_or("Cargo.toml has no [workspace]")?;
    let workspace_packages = workspace_package_names(workspace);
    let manifests = member_manifests(tree, workspace)?;
    let crates = manifests
        .iter()
        .map(package_name)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut edges = BTreeSet::new();
    let mut dirs = BTreeMap::new();
    for manifest in &manifests {
        let from = package_name(manifest)?;
        dirs.insert(from.clone(), manifest.0.trim_end_matches("/Cargo.toml").to_owned());
        for (to, kind) in dependencies(&manifest.1, &workspace_packages) {
            if crates.contains(&to) {
                edges.insert(Edge { from: from.clone(), to, kind });
            }
        }
    }
    Ok(CrateGraph { crates, edges, dirs })
}

fn parse(tree: &dyn Tree, path: &str) -> Result<Option<Table>, String> {
    let Some(text) = tree.read(path)? else { return Ok(None) };
    text.parse::<Table>()
        .map(Some)
        .map_err(|error| format!("parse {path}: {error}"))
}

/// Maps each `[workspace.dependencies]` key to the package it names, so a
/// `foo = { workspace = true }` entry resolves through a `package = "..."` rename.
fn workspace_package_names(workspace: &Table) -> BTreeMap<String, String> {
    let Some(table) = workspace.get("dependencies").and_then(Value::as_table) else {
        return BTreeMap::new();
    };
    table
        .iter()
        .map(|(key, value)| (key.clone(), renamed_package(value).unwrap_or(key).to_owned()))
        .collect()
}

fn member_manifests(tree: &dyn Tree, workspace: &Table) -> Result<Vec<Manifest>, String> {
    let members = workspace
        .get("members")
        .and_then(Value::as_array)
        .ok_or("[workspace] has no members")?;
    let excluded = excluded_dirs(workspace);
    let mut manifests = Vec::new();
    for member in members {
        let member = member.as_str().ok_or("a workspace member is not a string")?;
        let is_glob = member.ends_with("/*");
        for dir in member_dirs(tree, member)? {
            if is_glob && is_excluded(&dir, &excluded) {
                continue;
            }
            let path = format!("{dir}/Cargo.toml");
            if let Some(manifest) = parse(tree, &path)? {
                manifests.push((path, manifest));
            }
        }
    }
    Ok(manifests)
}

/// The `[workspace] exclude` entries as path components. Cargo drops a
/// directory a member glob matches when an entry is that directory or one of
/// its ancestors. A member listed by name stays a member.
fn excluded_dirs(workspace: &Table) -> Vec<Vec<&str>> {
    let entries = workspace.get("exclude").and_then(Value::as_array);
    let paths = entries
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    paths.map(path_components).collect()
}

fn is_excluded(dir: &str, excluded: &[Vec<&str>]) -> bool {
    let components = path_components(dir);
    excluded
        .iter()
        .any(|entry| components.starts_with(entry))
}

fn path_components(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect()
}

/// Expands a member entry. Only a literal directory and a trailing `/*` are
/// supported, which is every form the workspace uses.
fn member_dirs(tree: &dyn Tree, member: &str) -> Result<Vec<String>, String> {
    let Some(parent) = member.strip_suffix("/*") else {
        if member.contains(['*', '?', '[']) {
            return Err(format!("unsupported workspace member glob {member:?}"));
        }
        return Ok(vec![member.to_owned()]);
    };
    let names = tree.subdirectories(parent)?;
    Ok(names
        .into_iter()
        .map(|name| format!("{parent}/{name}"))
        .collect())
}

fn package_name((path, manifest): &Manifest) -> Result<String, String> {
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{path} has no package.name"))
}

/// Every dependency the manifest declares, including target-specific ones,
/// as the package name it resolves to.
fn dependencies(
    manifest: &Table,
    workspace_packages: &BTreeMap<String, String>,
) -> Vec<(String, DependencyKind)> {
    let targets = manifest.get("target").and_then(Value::as_table);
    let sections = std::iter::once(manifest)
        .chain(
            targets
                .into_iter()
                .flat_map(|targets| targets.values().filter_map(Value::as_table)),
        );
    let mut found = Vec::new();
    for section in sections {
        for (table_name, kind) in DependencyKind::TABLES {
            let Some(table) = section.get(table_name).and_then(Value::as_table) else { continue };
            found.extend(
                table
                    .iter()
                    .map(|(key, value)| (dependency_package(key, value, workspace_packages), kind)),
            );
        }
    }
    found
}

fn dependency_package(
    key: &str,
    value: &Value,
    workspace_packages: &BTreeMap<String, String>,
) -> String {
    if let Some(package) = renamed_package(value) {
        return package.to_owned();
    }
    let inherits = value.get("workspace").and_then(Value::as_bool) == Some(true);
    match workspace_packages.get(key) {
        Some(package) if inherits => package.clone(),
        _ => key.to_owned(),
    }
}

fn renamed_package(value: &Value) -> Option<&str> {
    value.get("package").and_then(Value::as_str)
}
