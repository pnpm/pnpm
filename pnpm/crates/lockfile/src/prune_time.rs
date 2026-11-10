//! The `time:` normalization pnpm applies on its way to disk.

use crate::{ImporterDepVersion, PkgName};
use serde_json::Value;
use std::collections::HashSet;

/// Dependency groups an importer records its direct dependencies under,
/// named as they appear in the serialized document.
const IMPORTER_DEPENDENCY_GROUPS: [&str; 3] =
    ["dependencies", "devDependencies", "optionalDependencies"];

/// Port of `pruneTimeInLockfile` (`lockfile/fs/src/lockfileFormatConverters.ts`):
/// drop every `time:` entry that is not one of the importers' direct
/// dependencies, so recording publish dates costs one entry per direct
/// dependency rather than one per resolved package.
///
/// `document` is a serialized lockfile. Values that are not
/// lockfile-shaped — the env lockfile, an `afterAllResolved` result that
/// dropped a section — pass through untouched.
pub fn prune_time(document: &mut Value) {
    if !document.get("time").is_some_and(Value::is_object) {
        return;
    }
    let direct_dep_paths = importer_dep_paths(document);
    let Some(Value::Object(time)) = document.get_mut("time") else { return };
    time.retain(|dep_path, _| direct_dep_paths.contains(dep_path));
}

/// Every depPath the importers depend on directly, peer suffix stripped —
/// the keys a `time:` entry is allowed to carry.
fn importer_dep_paths(document: &Value) -> HashSet<String> {
    let mut dep_paths = HashSet::new();
    let importers = document
        .get("importers")
        .or_else(|| document.get("projects"));
    let Some(Value::Object(importers)) = importers else { return dep_paths };
    for importer in importers.values() {
        for group in IMPORTER_DEPENDENCY_GROUPS {
            let Some(Value::Object(dependencies)) = importer.get(group) else { continue };
            dep_paths.extend(
                dependencies
                    .iter()
                    .filter_map(|(alias, dependency)| {
                        resolved_dep_path(alias, dependency, document)
                    }),
            );
        }
    }
    dep_paths
}

/// A catalog entry's specifier and version. In the next lockfile format the
/// entry can be the specifier alone, whose version follows from it.
fn catalog_entry(entry: &Value) -> Option<(String, String)> {
    if let Some(specifier) = entry.as_str() {
        let version = crate::implied_version::catalog_implied_version(specifier)?;
        return Some((specifier.to_string(), version.to_string()));
    }
    let text = |key: &str| Some(entry.get(key)?.as_str()?.to_string());
    Some((text("specifier")?, text("version")?))
}

/// The peer-suffix-stripped depPath one importer dependency entry resolves
/// to. An entry the reader cannot parse names no `time:` key. In the next
/// lockfile format, an entry can be the specifier alone, or a map without
/// `version`, whose version is implied, through `catalogs`, under
/// `settings` in that format, for a `catalog:` specifier. A `workspace:` specifier alone names an injected
/// or a linked workspace project, which has no `time:` entry, so it is not
/// looked up.
fn resolved_dep_path(alias: &str, dependency: &Value, document: &Value) -> Option<String> {
    let implied = |specifier: &str| {
        crate::implied_version::implied_version(
            alias,
            specifier,
            |catalog, alias| {
                let catalogs = document
                    .get("catalogs")
                    .or_else(|| document.get("settings")?.get("catalogs"))?;
                catalog_entry(catalogs.get(catalog)?.get(alias)?)
            },
            |_| None,
            |_| None,
        )
    };
    let version = match dependency {
        Value::String(specifier) => implied(specifier)?,
        _ => match dependency.get("version").and_then(Value::as_str) {
            Some(version) => version.to_string(),
            None => implied(dependency.get("specifier")?.as_str()?)?,
        },
    };
    let alias = PkgName::parse(alias).ok()?;
    let version = version.parse::<ImporterDepVersion>().ok()?;
    Some(
        version
            .resolved_key(&alias)?
            .without_peer()
            .to_string(),
    )
}

#[cfg(test)]
mod tests;
