use super::super::{DependencyGroup, OutdatedInWorkspace, OutdatedPackage};

fn json_key(
    package_name: &str,
    current: &str,
    github_action: bool,
    belongs_to: DependencyGroup,
    has_multiple: bool,
) -> String {
    if !has_multiple {
        return package_name.to_string();
    }
    let suffix = if github_action {
        " (github action)"
    } else {
        match belongs_to {
            DependencyGroup::Dev => " (dev)",
            DependencyGroup::Optional => " (optional)",
            DependencyGroup::Peer => " (peer)",
            DependencyGroup::Prod => "",
        }
    };
    if current.is_empty() {
        format!("{package_name}{suffix}")
    } else {
        format!("{package_name}@{current}{suffix}")
    }
}

pub(crate) fn render_json(outdated: &[OutdatedPackage], long: bool) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for pkg in outdated {
        *counts.entry(&pkg.package_name).or_default() += 1;
    }
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
        let key = json_key(
            &pkg.package_name,
            &pkg.current.to_string(),
            pkg.github_action,
            pkg.belongs_to,
            counts
                .get(pkg.package_name.as_str())
                .copied()
                .unwrap_or(0)
                > 1,
        );
        map.insert(key, entry);
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
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for entry in outdated {
        *counts.entry(&entry.package.package_name).or_default() += 1;
    }
    let mut map = serde_json::Map::new();
    for entry in outdated {
        let package = &entry.package;
        let value = recursive_entry_value(entry, long);
        let key = json_key(
            &package.package_name,
            &package.current.to_string(),
            package.github_action,
            package.belongs_to,
            counts
                .get(package.package_name.as_str())
                .copied()
                .unwrap_or(0)
                > 1,
        );
        map.insert(key, value);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize recursive outdated report to JSON")
}
