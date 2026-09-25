use super::{
    super::{OutdatedInWorkspace, OutdatedPackage},
    dependency_type_label,
};
use std::collections::HashMap;

/// JSON output is keyed by package name. A name that occurs more than once in
/// the report, such as one package installed at several versions across a
/// workspace, is keyed by name, current version, and dependency type instead,
/// so that no entry overwrites another.
struct JsonKeys<'a> {
    counts: HashMap<&'a str, usize>,
}

impl<'a> JsonKeys<'a> {
    fn new(packages: impl Iterator<Item = &'a OutdatedPackage>) -> Self {
        let mut counts = HashMap::new();
        for pkg in packages {
            *counts.entry(pkg.package_name.as_str()).or_default() += 1;
        }
        JsonKeys { counts }
    }

    fn key(&self, pkg: &OutdatedPackage) -> String {
        if self.counts
            .get(pkg.package_name.as_str())
            .is_none_or(|&count| count == 1)
        {
            return pkg.package_name.clone();
        }
        let suffix = dependency_type_label(pkg)
            .map(|label| format!(" ({label})"))
            .unwrap_or_default();
        format!("{}@{}{suffix}", pkg.package_name, pkg.current)
    }
}

pub(crate) fn render_json(outdated: &[OutdatedPackage], long: bool) -> String {
    let keys = JsonKeys::new(outdated.iter());
    let mut map = serde_json::Map::new();
    for pkg in outdated {
        let dependency_type: &'static str =
            if pkg.github_action { "githubAction" } else { pkg.belongs_to.into() };
        let mut entry = serde_json::json!({
            "current": pkg.current.to_string(),
            "latest": pkg.target.to_string(),
            "wanted": pkg.wanted.to_string(),
            "isDeprecated": pkg.metadata.deprecated.is_some(),
            "dependencyType": dependency_type,
        });
        if long {
            entry["latestManifest"] = serde_json::json!({
                "name": pkg.package_name,
                "version": pkg.target.to_string(),
                "deprecated": pkg.metadata.deprecated,
                "homepage": pkg.metadata.homepage,
            });
        }
        map.insert(keys.key(pkg), entry);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize outdated report to JSON")
}

fn recursive_entry_value(entry: &OutdatedInWorkspace, long: bool) -> serde_json::Value {
    let package = &entry.package;
    let dependency_type: &'static str =
        if package.github_action { "githubAction" } else { package.belongs_to.into() };
    let mut value = serde_json::json!({
        "current": package.current.to_string(),
        "latest": package.target.to_string(),
        "wanted": package.wanted.to_string(),
        "isDeprecated": package.metadata.deprecated.is_some(),
        "dependencyType": dependency_type,
        "dependentPackages": entry.dependents.iter().map(|dependent| serde_json::json!({
            "name": dependent.name,
            "location": dependent.location.to_string_lossy(),
        })).collect::<Vec<_>>(),
    });
    if long {
        value["latestManifest"] = serde_json::json!({
            "name": package.package_name,
            "version": package.target.to_string(),
            "deprecated": package.metadata.deprecated,
            "homepage": package.metadata.homepage,
        });
    }
    value
}

pub(crate) fn render_recursive_json(outdated: &[OutdatedInWorkspace], long: bool) -> String {
    let keys = JsonKeys::new(outdated.iter().map(|entry| &entry.package));
    let mut map = serde_json::Map::new();
    for entry in outdated {
        map.insert(keys.key(&entry.package), recursive_entry_value(entry, long));
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize recursive outdated report to JSON")
}
