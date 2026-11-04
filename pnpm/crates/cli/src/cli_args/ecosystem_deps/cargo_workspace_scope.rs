#[cfg(test)]
pub(super) mod tests;

use super::cargo_manifest_ops::{DeclaredCargoDep, declared_cargo_deps, same_declared_dep};
use crate::cargo_deps::CargoWorkspaceDependency;
use miette::{IntoDiagnostic, Result, WrapErr};
use semver::VersionReq;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

/// A declaration's version requirement and the features it enables.
type RequirementFeatures = (VersionReq, Vec<String>);

/// The Cargo workspace a `pnpm update` edits.
pub(crate) struct CargoWorkspaceScope {
    pub(crate) root: PathBuf,
    pub(crate) declared: Vec<DeclaredCargoDep>,
    /// The registry dependency declarations of the workspace's packages, as
    /// `cargo metadata` reports them, keyed by package name and alias.
    declarations: HashMap<(String, String), Vec<RequirementFeatures>>,
    root_manifest: RootManifest,
}

impl CargoWorkspaceScope {
    /// The features the workspace enables on `dep`, from every declaration
    /// that resolves through it: the declaration itself, the members
    /// inheriting it, and their target-specific tables.
    pub(crate) fn enabled_features(&self, dep: &DeclaredCargoDep) -> Vec<String> {
        let Some(declarations) =
            self.declarations.get(&(dep.package_name.clone(), dep.alias.clone()))
        else {
            return Vec::new();
        };
        let features: BTreeSet<&String> = declarations
            .iter()
            .filter(|(req, _)| *req == dep.version_req)
            .flat_map(|(_, features)| features)
            .collect();
        features.into_iter().cloned().collect()
    }

    /// Whether the workspace also declares `dep`'s package under another
    /// name, so `dep` is a renamed alias kept on a line of its own.
    pub(crate) fn has_sibling_alias(&self, dep: &DeclaredCargoDep) -> bool {
        let is_sibling =
            |(name, alias): &(String, String)| *name == dep.package_name && *alias != dep.alias;
        dep.alias != dep.package_name
            && (self.declarations.keys().any(is_sibling)
                || self.root_manifest.workspace_aliases.iter().any(is_sibling))
    }

    pub(crate) fn is_patched(&self, package_name: &str) -> bool {
        self.root_manifest.patched.contains(package_name)
    }

    /// Record the declarations `pnpm update` can target: those resolved
    /// from the configured registry, not from an alternate registry, a
    /// path, or git.
    fn add_declarations(&mut self, dependencies: Vec<CargoWorkspaceDependency>) {
        for dependency in dependencies {
            if !is_registry_declaration(&dependency) {
                continue;
            }
            let Ok(req) = VersionReq::parse(&dependency.req) else {
                continue;
            };
            let alias = dependency.rename.unwrap_or_else(|| dependency.name.clone());
            self.declarations
                .entry((dependency.name, alias))
                .or_default()
                .push((req, dependency.features));
        }
    }
}

fn is_registry_declaration(dependency: &CargoWorkspaceDependency) -> bool {
    dependency.registry.is_none()
        && dependency.source
            .as_deref()
            .is_some_and(|source| source.starts_with("registry+") || source.starts_with("sparse+"))
}

pub(crate) async fn resolve_cargo_workspace_scope(
    manifest_path: &Path,
    declared: &[DeclaredCargoDep],
    checkout: Option<&Path>,
) -> Result<CargoWorkspaceScope> {
    let (_, metadata) = crate::cargo_deps::workspace_metadata(manifest_path, checkout).await?;
    let cargo_root = crate::cargo_deps::canonical_cargo_path(&metadata.workspace_root)?;
    let root_manifest = read_root_manifest(&cargo_root.join("Cargo.toml"))?;
    let mut scope = CargoWorkspaceScope {
        root: cargo_root,
        declared: declared.to_vec(),
        declarations: HashMap::new(),
        root_manifest,
    };
    for package in metadata.packages {
        let canonical = crate::cargo_deps::canonical_cargo_path(&package.manifest_path)?;
        if let Some(dir) = canonical
            .parent()
            .filter(|dir| dir.starts_with(&scope.root))
        {
            extend_unique_declared(
                &mut scope.declared,
                declared_cargo_deps(dir, Some(&scope.root))?,
                &scope.root,
            );
            scope.add_declarations(package.dependencies);
        }
    }
    Ok(scope)
}

/// What `pnpm update` reads from the workspace's root manifest.
struct RootManifest {
    /// The crates a crates.io `[patch]` table replaces.
    patched: BTreeSet<String>,
    /// `(package name, alias)` of each registry entry in
    /// `[workspace.dependencies]`, including entries no member inherits.
    workspace_aliases: BTreeSet<(String, String)>,
}

fn read_root_manifest(root_manifest_path: &Path) -> Result<RootManifest> {
    let contents = fs::read_to_string(root_manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", root_manifest_path.display()))?;
    let manifest: toml::Table = toml::from_str(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", root_manifest_path.display()))?;
    let patched = manifest
        .get("patch")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(key, _)| pnpm_cargo_resolver::patches_crates_io(key))
        .filter_map(|(_, table)| table.as_table())
        .flatten()
        .map(|(alias, entry)| package_name(alias, entry))
        .collect();
    let workspace_aliases = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(_, entry)| is_registry_entry(entry))
        .map(|(alias, entry)| (package_name(alias, entry), alias.clone()))
        .collect();
    Ok(RootManifest { patched, workspace_aliases })
}

fn package_name(alias: &str, entry: &toml::Value) -> String {
    entry
        .get("package")
        .and_then(toml::Value::as_str)
        .unwrap_or(alias)
        .to_string()
}

fn is_registry_entry(entry: &toml::Value) -> bool {
    entry
        .as_table()
        .is_none_or(|table| {
            !["git", "path", "registry"]
                .iter()
                .any(|key| table.contains_key(*key))
        })
}

fn extend_unique_declared(
    all_declared: &mut Vec<DeclaredCargoDep>,
    deps: Vec<DeclaredCargoDep>,
    cargo_root: &Path,
) {
    for dep in deps {
        if dep.manifest_path.starts_with(cargo_root)
            && !all_declared
                .iter()
                .any(|existing| existing.group == dep.group && same_declared_dep(existing, &dep))
        {
            all_declared.push(dep);
        }
    }
}
