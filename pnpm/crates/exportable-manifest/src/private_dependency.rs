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

/// The registry names of the private workspace packages that `original`'s
/// `dependencies` point at through a `workspace:` specifier, once a
/// `catalog:` entry is resolved. Pass them to
/// [`reject_private_workspace_dependencies`] after the `beforePacking` hooks
/// have settled the published manifest.
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
        if let Some(package) = private_workspace_package(dep_name, &spec, dir, opts)? {
            private.push(package);
        }
    }
    Ok(private)
}

/// Fail when a `dependencies` entry of the publishable manifest `published`
/// installs one of the `private_packages` from the registry, under any key.
/// The entries named in `bundled` ship inside the tarball and are not
/// checked, and a private manifest is never published.
pub fn reject_private_workspace_dependencies(
    published: &Value,
    private_packages: &[String],
    bundled: &[String],
) -> Result<(), PrivateWorkspaceDependencyError> {
    let Some(published) = published.as_object() else {
        return Ok(());
    };
    if private_packages.is_empty() || is_private(published) {
        return Ok(());
    }
    let Some(dependencies) = published.get("dependencies").and_then(Value::as_object) else {
        return Ok(());
    };
    let rejected = dependencies
        .iter()
        .filter(|(dep_name, _)| !bundled.contains(dep_name))
        .filter_map(|(dep_name, spec)| Some((dep_name.as_str(), spec.as_str()?)))
        .flat_map(|(dep_name, spec)| registry_packages(dep_name, spec))
        .flatten()
        .find(|package| {
            private_packages
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
        .map(
            |manifest| {
                if manifest.name.is_empty() { dep_name.to_string() } else { manifest.name }
            },
        ))
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
