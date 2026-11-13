//! Version extraction from npm's lockfile formats.

use pnpm_resolving_resolver_base::is_valid_semver_range;
use serde_json::{Map, Value};

use crate::{VersionsByPackageName, add_version, split_descriptor};

const NODE_MODULES_PREFIX: &str = "node_modules/";

/// Collect every version an npm lockfile pins.
///
/// Lockfile versions 1 and 2 nest a `dependencies` tree whose entries
/// each carry a concrete `version`. Version 3 replaced that with a flat
/// `packages` map keyed by the package's path under `node_modules`,
/// where an entry's own `dependencies` map holds the ranges its manifest
/// asked for. Those ranges are collected as well, because npm writes an
/// exact version there whenever the manifest pinned one, and for a
/// transitive dependency that is the only record of the version npm
/// chose.
///
/// A lockfile without a `lockfileVersion` is read in the flat format.
pub fn collect_npm_lockfile_versions(lockfile: &Value, versions: &mut VersionsByPackageName) {
    if is_nested_format(lockfile) {
        collect_from_dependency_tree(lockfile, versions);
        return;
    }
    for field in ["dependencies", "packages"] {
        if let Some(packages) = lockfile.get(field).and_then(Value::as_object) {
            collect_from_flat_packages(packages, versions);
        }
    }
}

fn is_nested_format(lockfile: &Value) -> bool {
    lockfile
        .get("lockfileVersion")
        .and_then(Value::as_f64)
        .is_some_and(|lockfile_version| lockfile_version < 3.0)
}

fn collect_from_dependency_tree(root: &Value, versions: &mut VersionsByPackageName) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let Some(dependencies) = node.get("dependencies").and_then(Value::as_object) else {
            continue;
        };
        for (name, entry) in dependencies {
            if let Some(version) = entry.get("version").and_then(Value::as_str) {
                add_resolved_npm_version(versions, name, version);
            }
            pending.push(entry);
        }
    }
}

fn collect_from_flat_packages(packages: &Map<String, Value>, versions: &mut VersionsByPackageName) {
    let mut pending = vec![packages];
    while let Some(packages) = pending.pop() {
        for (key, entry) in packages {
            collect_from_flat_entry(key, entry, versions, &mut pending);
        }
    }
}

/// Record one flat-format entry's own version and the ranges it declares, and
/// queue the packages nested under it.
fn collect_from_flat_entry<'a>(
    key: &str,
    entry: &'a Value,
    versions: &mut VersionsByPackageName,
    pending: &mut Vec<&'a Map<String, Value>>,
) {
    let key_name = package_name_from_key(key);
    if !key_name.is_empty()
        && let Some(version) = entry.get("version").and_then(Value::as_str)
    {
        let entry_name = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(key_name);
        add_resolved_npm_version(versions, entry_name, version);
    }
    if let Some(nested) = entry.get("packages").and_then(Value::as_object) {
        pending.push(nested);
    }
    let Some(dependencies) = entry.get("dependencies").and_then(Value::as_object) else {
        return;
    };
    for (name, range) in dependencies {
        if let Some(range) = range.as_str() {
            add_resolved_npm_version(versions, name, range);
        }
    }
}

/// Unwrap an `npm:<target>@<version>` alias value into the target package name
/// and version.
fn add_resolved_npm_version(versions: &mut VersionsByPackageName, name: &str, spec: &str) {
    let Some(npm_spec) = spec.strip_prefix("npm:") else {
        add_version(versions, name, spec);
        return;
    };
    if !name.is_empty() && is_valid_semver_range(npm_spec) {
        add_version(versions, name, npm_spec);
        return;
    }
    if let Some((target, target_spec)) = split_descriptor(npm_spec) {
        add_version(versions, target, target_spec);
    }
}

/// The package name is whatever follows the last `node_modules/` in a
/// flat-format key, leaving the root project's empty key empty.
fn package_name_from_key(key: &str) -> &str {
    match key.rfind(NODE_MODULES_PREFIX) {
        Some(index) => &key[index + NODE_MODULES_PREFIX.len()..],
        None => key,
    }
}

#[cfg(test)]
mod tests;
