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

/// The names in `original`'s `dependencies` whose specifier, once a
/// `catalog:` entry is resolved, is a `workspace:` specifier for a private
/// workspace package. Pass them to [`reject_private_workspace_dependencies`]
/// after the `beforePacking` hooks have settled the published manifest.
pub fn private_workspace_dependencies(
    dir: &Path,
    original: &Value,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<Vec<String>, CreateExportableManifestError> {
    let Some(dependencies) = original.get("dependencies").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut private = Vec::new();
    for (dep_name, spec) in dependencies {
        let Some(spec) = spec.as_str() else {
            continue;
        };
        let spec = replace_catalog_protocol(dep_name.clone(), spec, dir, opts)?;
        if is_private_workspace_package(dep_name, &spec, dir, opts)? {
            private.push(dep_name.clone());
        }
    }
    Ok(private)
}

/// Fail when the publishable manifest `published` still lists one of the
/// `private_dependencies` in `dependencies` for the registry package it named
/// in `exported`, the manifest before the `beforePacking` hooks ran. A hook may
/// drop such a dependency or point it at another package, but a new range for
/// the same package is still rejected. A private manifest
/// is never published, so it is not rejected. The caller leaves out the
/// dependencies the tarball bundles.
pub fn reject_private_workspace_dependencies(
    exported: &Value,
    published: &Value,
    private_dependencies: &[String],
) -> Result<(), PrivateWorkspaceDependencyError> {
    let Some(published) = published.as_object() else {
        return Ok(());
    };
    if is_private(published) {
        return Ok(());
    }
    let Some(dependencies) = published.get("dependencies").and_then(Value::as_object) else {
        return Ok(());
    };
    let exported = exported.get("dependencies");
    let rejected = private_dependencies
        .iter()
        .find(|dep_name| {
            let package = registry_package(dep_name, dependencies.get(dep_name.as_str()));
            package.is_some()
                && package
                    == registry_package(
                        dep_name,
                        exported.and_then(|exported| exported.get(dep_name.as_str())),
                    )
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
        dep_name: dep_name.clone(),
    })
}

/// Whether `spec` is a `workspace:` specifier whose target package, resolved
/// the way the workspace-protocol rewrite resolves it, is private.
fn is_private_workspace_package(
    dep_name: &str,
    spec: &str,
    dir: &Path,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<bool, CreateExportableManifestError> {
    let lookup = WorkspacePackageLookup {
        packages: opts.workspace_packages,
        prefer_workspace_for: opts.bumped_workspace_packages,
    };
    let target = workspace_target_manifest(dep_name, spec, dir, opts.modules_dir, lookup)
        .map_err(CreateExportableManifestError::ReplaceWorkspaceProtocol)?;
    Ok(target.is_some_and(|manifest| manifest.private))
}

/// The registry package a published specifier installs: the `npm:` alias
/// target, or `dep_name` for a plain range or tag. `None` for a specifier
/// that does not install from the registry, such as a path, URL, or git
/// specifier.
fn registry_package<'a>(dep_name: &'a str, spec: Option<&'a Value>) -> Option<&'a str> {
    let spec = spec?.as_str()?;
    if is_local_filesystem_specifier(spec) {
        return None;
    }
    if let Some(aliased) = spec.strip_prefix("npm:") {
        return Some(match aliased.rfind('@') {
            Some(index) if index > 0 => &aliased[..index],
            _ => aliased,
        });
    }
    (!spec.contains(':') && !spec.contains('/')).then_some(dep_name)
}

fn is_private(manifest: &Map<String, Value>) -> bool {
    manifest.get("private") == Some(&Value::Bool(true))
}

#[cfg(test)]
mod tests;
