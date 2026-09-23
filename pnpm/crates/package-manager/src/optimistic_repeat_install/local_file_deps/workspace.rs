//! Workspace dependency matching for repeat install.

use crate::optimistic_repeat_install::{
    CatalogAnchor, CatalogResolutionResult, Catalogs, PackageManifest, WantedDependency,
    resolve_from_catalog,
};
use std::{collections::HashMap, path::PathBuf};

pub(super) type WorkspacePackageMap<'a> = HashMap<&'a str, Option<node_semver::Version>>;

pub(super) fn collect_workspace_packages<'a>(
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
) -> WorkspacePackageMap<'a> {
    project_manifests
        .iter()
        .filter_map(|(_, manifest)| {
            let name = manifest.value().get("name")?.as_str()?;
            let version = manifest
                .value()
                .get("version")
                .and_then(serde_json::Value::as_str)
                .and_then(|raw_ver| raw_ver.parse::<node_semver::Version>().ok());
            Some((name, version))
        })
        .collect()
}

pub(super) fn dependency_is_workspace_or_injected(
    workspace_packages: &WorkspacePackageMap<'_>,
    inject_workspace_packages: bool,
    catalogs: &Catalogs,
    manifest: &serde_json::Value,
    alias: &str,
    spec: &serde_json::Value,
) -> bool {
    if dependency_is_injected(manifest, alias) {
        return true;
    }
    if !inject_workspace_packages {
        return false;
    }
    let Some(spec_str) = spec.as_str() else {
        return false;
    };
    let resolved_spec = if spec_str.starts_with("catalog:") {
        resolve_catalog_spec(catalogs, alias, spec_str)
    } else {
        Some(spec_str.to_string())
    };
    let Some(actual_spec) = resolved_spec else {
        return false;
    };
    workspace_dep_matches(workspace_packages, alias, &actual_spec)
}

fn dependency_is_injected(manifest: &serde_json::Value, name: &str) -> bool {
    manifest
        .get("dependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .and_then(|meta| meta.get(name))
        .and_then(|entry| entry.get("injected"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn resolve_catalog_spec(catalogs: &Catalogs, alias: &str, spec: &str) -> Option<String> {
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
        CatalogAnchor::AsWritten,
    ) {
        CatalogResolutionResult::Found(found) => Some(found.resolution.specifier),
        _ => None,
    }
}

fn workspace_dep_matches(
    workspace_packages: &WorkspacePackageMap<'_>,
    alias: &str,
    spec: &str,
) -> bool {
    if let Some(ws_spec) = spec.strip_prefix("workspace:") {
        return ws_spec_matches_workspace(workspace_packages, alias, ws_spec);
    }
    let (target_name, range_str) = parse_target_name_and_range(alias, spec);
    let Some(target_ver) = workspace_packages.get(target_name) else {
        return false;
    };
    let Ok(range) = range_str.parse::<node_semver::Range>() else {
        return true;
    };
    match target_ver {
        Some(version) => range.satisfies(version),
        None => true,
    }
}

fn is_workspace_path(ws_spec: &str) -> bool {
    let is_windows_drive = {
        let mut chars = ws_spec.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphabetic()) && chars.next() == Some(':')
    };
    ws_spec.starts_with('.')
        || ws_spec.starts_with('/')
        || ws_spec.starts_with("~/")
        || is_windows_drive
}

fn ws_spec_matches_workspace(
    workspace_packages: &WorkspacePackageMap<'_>,
    alias: &str,
    ws_spec: &str,
) -> bool {
    if is_workspace_path(ws_spec) {
        return true;
    }
    let (target_name, range_str) = match ws_spec.rfind('@') {
        Some(idx) if idx > 0 => (&ws_spec[..idx], &ws_spec[idx + 1..]),
        _ => (alias, ws_spec),
    };
    if !workspace_packages.contains_key(target_name) {
        return false;
    }
    let parsed_range_str = match range_str {
        "*" | "^" | "~" | "" => return true,
        other => other,
    };
    let Ok(range) = parsed_range_str.parse::<node_semver::Range>() else {
        return true;
    };
    match workspace_packages.get(target_name).and_then(Option::as_ref) {
        Some(version) => range.satisfies(version),
        None => true,
    }
}

fn parse_target_name_and_range<'a>(alias: &'a str, spec: &'a str) -> (&'a str, &'a str) {
    let Some(aliased) = spec.strip_prefix("npm:") else {
        return (alias, spec);
    };
    if aliased.parse::<node_semver::Range>().is_ok() {
        return (alias, aliased);
    }
    match aliased.rfind('@') {
        Some(idx) if idx > 0 => (&aliased[..idx], &aliased[idx + 1..]),
        _ => (aliased, "*"),
    }
}
