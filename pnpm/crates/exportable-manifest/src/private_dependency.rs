//! Rejects a publishable manifest whose `dependencies` name a private
//! workspace package. The published manifest would point at a version
//! that never reaches the registry, so nobody could install it.

use crate::{
    create::{
        CreateExportableManifestError,
        CreateExportableManifestOptions,
    },
    replace::{
        ReplaceWorkspaceProtocolError,
        installed_modules_dir,
        strip_workspace_relative_prefix,
    },
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use pnpm_workspace_spec::WorkspaceSpec;
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

/// Fail when the publishable manifest `original` lists a private workspace
/// package in `dependencies`. A private manifest is never published, and a
/// bundled dependency ships inside the tarball, so neither is checked.
pub(crate) fn check_private_workspace_dependencies(
    dir: &Path,
    original: &Map<String, Value>,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<(), CreateExportableManifestError> {
    if is_private(original) {
        return Ok(());
    }
    let Some(dependencies) = original.get("dependencies").and_then(Value::as_object) else {
        return Ok(());
    };
    for (dep_name, spec) in dependencies {
        let Some(spec) = spec.as_str() else {
            continue;
        };
        if is_bundled(original, dep_name)
            || !is_private_workspace_package(dep_name, spec, dir, opts)?
        {
            continue;
        }
        let package_name = original
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(CreateExportableManifestError::PrivateWorkspaceDependency(
            PrivateWorkspaceDependencyError {
                package_name: package_name.to_string(),
                dep_name: dep_name.clone(),
            },
        ));
    }
    Ok(())
}

/// Whether `spec` is a `workspace:` specifier whose target package is private.
/// The target is read where the workspace-protocol rewrite reads it: the
/// relative path, or the installed copy, falling back to the workspace
/// packages for a dependency that is not installed.
fn is_private_workspace_package(
    dep_name: &str,
    spec: &str,
    dir: &Path,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<bool, CreateExportableManifestError> {
    let Some(workspace_spec) = WorkspaceSpec::parse(spec) else {
        return Ok(false);
    };
    let relative = strip_workspace_relative_prefix(spec);
    let target_dir = match relative {
        Some(relative) => dir.join(relative),
        None => installed_modules_dir(dir, opts.modules_dir).join(dep_name),
    };
    let manifest = safe_read_package_json_from_dir(&target_dir)
        .map_err(|error| {
            CreateExportableManifestError::ReplaceWorkspaceProtocol(
                ReplaceWorkspaceProtocolError::ReadManifest(error),
            )
        })?;
    if let Some(manifest) = manifest {
        return Ok(manifest.as_object().is_some_and(is_private));
    }
    if relative.is_some() {
        return Ok(false);
    }
    let target_name = workspace_spec.alias.as_deref().unwrap_or(dep_name);
    Ok(opts.workspace_packages
        .and_then(|packages| packages.get(target_name))
        .is_some_and(|package| package.private))
}

fn is_private(manifest: &Map<String, Value>) -> bool {
    manifest.get("private") == Some(&Value::Bool(true))
}

fn is_bundled(manifest: &Map<String, Value>, dep_name: &str) -> bool {
    ["bundleDependencies", "bundledDependencies"]
        .iter()
        .filter_map(|field| manifest.get(*field))
        .any(|bundled| match bundled {
            Value::Bool(all) => *all,
            Value::Array(names) => names
                .iter()
                .any(|name| name.as_str() == Some(dep_name)),
            _ => false,
        })
}

#[cfg(test)]
mod tests;
