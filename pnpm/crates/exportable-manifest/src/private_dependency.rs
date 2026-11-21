//! Rejects a publishable manifest whose `dependencies` name a private
//! workspace package. The published manifest would point at a version
//! that never reaches the registry, so nobody could install it.

use crate::{
    create::{
        CreateExportableManifestError,
        CreateExportableManifestOptions,
        replace_catalog_protocol,
    },
    replace::{
        WorkspacePackageLookup,
        workspace_target_manifest,
    },
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_local_spec::is_local_filesystem_specifier;
use serde_json::{
    Map,
    Value,
};
use std::path::Path;

const DEPENDENCY_FIELDS: &[&str] =
    &["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"];

/// A publishable package lists a private workspace package in
/// `dependencies`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(r#""{package_name}" depends on the private workspace package "{dep_name}""#)]
#[diagnostic(
    code(ERR_PNPM_PRIVATE_WORKSPACE_DEPENDENCY),
    help(
        r#"A private package is never published. Move "{dep_name}" to devDependencies or bundleDependencies, or remove "private": true from its package.json."#
    )
)]
pub struct PrivateWorkspaceDependencyError {
    #[error(not(source))]
    pub package_name: String,
    pub dep_name: String,
}

/// The private workspace packages a manifest points at through `workspace:`
/// specifiers, collected by [`private_workspace_dependencies`] before the
/// `beforePacking` hooks run and checked by [`Self::reject`] after them.
#[derive(Debug, Default)]
pub struct PrivateWorkspaceDependencies {
    /// Registry names of the private targets.
    packages: Vec<String>,
    /// Keys of `dependencies` whose `workspace:` specifier names one of
    /// `packages`.
    keys: Vec<String>,
    /// The `dependencies` of the exported manifest, before the hooks.
    exported: Map<String, Value>,
}

/// Collect the private workspace packages that any of `original`'s dependency
/// fields point at through a `workspace:` specifier, once a `catalog:` entry
/// is resolved. Every field counts, because a hook can move an entry into
/// `dependencies`. Record the exported manifest with
/// [`PrivateWorkspaceDependencies::record_exported`] before the hooks run.
pub fn private_workspace_dependencies(
    dir: &Path,
    original: &Value,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<PrivateWorkspaceDependencies, CreateExportableManifestError> {
    let dependencies = DEPENDENCY_FIELDS
        .iter()
        .filter_map(|field| {
            let entries = original.get(*field).and_then(Value::as_object)?;
            Some(
                entries
                    .iter()
                    .map(move |entry| (*field, entry)),
            )
        });
    let mut private = PrivateWorkspaceDependencies::default();
    for (field, (dep_name, spec)) in dependencies.flatten() {
        let Some(spec) = spec.as_str() else {
            continue;
        };
        let spec = replace_catalog_protocol(dep_name.clone(), spec, dir, opts)?;
        let Some(package) = private_workspace_package(dep_name, &spec, dir, opts)? else {
            continue;
        };
        if !private.packages.contains(&package) {
            private.packages.push(package);
        }
        if field == "dependencies" {
            private.keys.push(dep_name.clone());
        }
    }
    Ok(private)
}

impl PrivateWorkspaceDependencies {
    /// Keep the `dependencies` of the manifest pack exported, before the
    /// `beforePacking` hooks change it.
    pub fn record_exported(&mut self, exported: &Value) {
        if let Some(dependencies) = exported.get("dependencies").and_then(Value::as_object) {
            self.exported.clone_from(dependencies);
        }
    }

    /// Fail when a `dependencies` entry of the publishable manifest
    /// `published` installs one of the private packages from the registry.
    /// An entry counts when it was a private `workspace:` dependency, or when
    /// a hook added or changed it, so an untouched registry dependency that
    /// shares the private package's name is allowed. Not checked: the entries
    /// named in `bundled`, which ship inside the tarball, entries shadowed by
    /// an `optionalDependencies` entry of the same name, which is installed
    /// instead, and a private manifest, which is never published.
    pub fn reject(
        &self,
        published: &Value,
        bundled: &[String],
    ) -> Result<(), PrivateWorkspaceDependencyError> {
        let Some(published) = published.as_object() else {
            return Ok(());
        };
        if self.packages.is_empty() || is_private(published) {
            return Ok(());
        }
        let Some(dependencies) = published.get("dependencies").and_then(Value::as_object) else {
            return Ok(());
        };
        let optional = published.get("optionalDependencies").and_then(Value::as_object);
        let rejected = dependencies
            .iter()
            .filter(|(dep_name, _)| !bundled.contains(dep_name))
            .filter(|(dep_name, _)| {
                !optional.is_some_and(|optional| optional.contains_key(dep_name.as_str()))
            })
            .filter(|(dep_name, spec)| {
                self.keys.contains(dep_name) || self.exported.get(dep_name.as_str()) != Some(spec)
            })
            .filter_map(|(dep_name, spec)| Some((dep_name.as_str(), spec.as_str()?)))
            .flat_map(|(dep_name, spec)| registry_packages(dep_name, spec))
            .flatten()
            .find(|package| {
                self.packages
                    .iter()
                    .any(|private| private == package)
            });
        let Some(dep_name) = rejected else {
            return Ok(());
        };
        let package_name = published
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Err(PrivateWorkspaceDependencyError {
            package_name: package_name.to_string(),
            dep_name: dep_name.to_string(),
        })
    }
}

/// The registry name of the private workspace package `spec` points at, when
/// it is a `workspace:` specifier whose target, resolved the way the
/// workspace-protocol rewrite resolves it, is private.
fn private_workspace_package(
    dep_name: &str,
    spec: &str,
    dir: &Path,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<Option<String>, CreateExportableManifestError> {
    let lookup = WorkspacePackageLookup {
        packages: opts.workspace_packages,
        prefer_workspace_for: opts.bumped_workspace_packages,
    };
    let target = workspace_target_manifest(dep_name, spec, dir, opts.modules_dir, lookup)
        .map_err(CreateExportableManifestError::ReplaceWorkspaceProtocol)?;
    Ok(target
        .filter(|manifest| manifest.private)
        .map(|manifest| {
            manifest.publish_name.unwrap_or_else(|| {
                if manifest.name.is_empty() { dep_name.to_string() } else { manifest.name }
            })
        }))
}

/// The registry packages a published dependency may install, read
/// conservatively so no form that reaches the private package slips by:
/// - `npm:<name>@<range>` names `<name>`.
/// - A bare `npm:<x>` is `<x>` at the default tag, or a range for `dep_name`,
///   so it names both.
/// - A local path names none, and so does a URL, a git source, or another
///   protocol: a first `||` member with a `:` or `/`.
/// - Anything else, a range or a tag, is read as `dep_name`, even when a
///   later union member names another protocol.
fn registry_packages<'a>(dep_name: &'a str, spec: &'a str) -> [Option<&'a str>; 2] {
    if is_local_filesystem_specifier(spec) {
        return [None, None];
    }
    match spec.strip_prefix("npm:") {
        Some(aliased) => match aliased.rfind('@') {
            Some(index) if index > 0 => [Some(&aliased[..index]), None],
            _ => [Some(aliased), Some(dep_name)],
        },
        None if names_another_source(spec) => [None, None],
        None => [Some(dep_name), None],
    }
}

fn names_another_source(spec: &str) -> bool {
    let first_member = spec
        .split("||")
        .next()
        .unwrap_or_default();
    first_member.contains(':') || first_member.contains('/')
}

fn is_private(manifest: &Map<String, Value>) -> bool {
    manifest.get("private") == Some(&Value::Bool(true))
}

#[cfg(test)]
mod tests;
