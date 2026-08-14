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
    let Some(Value::Object(importers)) = document.get("importers") else { return dep_paths };
    for importer in importers.values() {
        for group in IMPORTER_DEPENDENCY_GROUPS {
            let Some(Value::Object(dependencies)) = importer.get(group) else { continue };
            for (alias, dependency) in dependencies {
                let Some(version) = dependency.get("version").and_then(Value::as_str) else {
                    continue;
                };
                let (Ok(alias), Ok(version)) =
                    (PkgName::parse(alias.as_str()), version.parse::<ImporterDepVersion>())
                else {
                    continue;
                };
                let Some(dep_path) = version.resolved_key(&alias) else { continue };
                dep_paths.insert(dep_path.without_peer().to_string());
            }
        }
    }
    dep_paths
}

#[cfg(test)]
mod tests;
