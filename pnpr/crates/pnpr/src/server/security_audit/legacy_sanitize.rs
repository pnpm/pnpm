use std::collections::HashMap;

use serde_json::{Map, Value};

use super::{
    PackageDestination,
    legacy_nodes::{
        MAX_DEPENDENCY_DEPTH, PACKAGE_SPEC_FIELDS, entry_bool, node_modules_segments,
        parse_name_and_spec, parse_npm_alias,
    },
};

const REDACTED_ROOT_NAME: &str = "pnpr-audit";

pub(super) fn build_upstream_legacy_payload(
    payload: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut sanitized = Map::new();
    copy_sanitized_root_identity(payload, upstream_name, destinations, &mut sanitized);
    if let Some(lockfile_version) = payload.get("lockfileVersion") {
        sanitized.insert("lockfileVersion".to_string(), lockfile_version.clone());
    }
    let mut filtered_requires = payload
        .get("requires")
        .and_then(Value::as_object)
        .map(|req| filter_spec_map(req, upstream_name, destinations));

    if let Some(deps) = payload.get("dependencies").and_then(Value::as_object) {
        let filtered_deps = filter_dependencies_map(deps, upstream_name, destinations, 0);
        if let Some(req_map) = filtered_requires.as_mut() {
            backfill_promoted_requires(req_map, &filtered_deps);
        }
        sanitized.insert("dependencies".to_string(), Value::Object(filtered_deps));
    } else if let Some(Value::Bool(flag)) = payload.get("requires") {
        sanitized.insert("requires".to_string(), Value::Bool(*flag));
    }
    if let Some(req_map) = filtered_requires {
        sanitized.insert("requires".to_string(), Value::Object(req_map));
    }
    if let Some(packages) = payload.get("packages").and_then(Value::as_object) {
        let filtered_pkgs = filter_packages_map(packages, upstream_name, destinations);
        sanitized.insert("packages".to_string(), Value::Object(filtered_pkgs));
    }
    copy_sanitized_arrays_and_metadata(payload, upstream_name, destinations, &mut sanitized);
    sanitized
}

fn copy_sanitized_root_identity(
    payload: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
    sanitized: &mut Map<String, Value>,
) {
    let Some(root_name) = payload.get("name").and_then(Value::as_str) else {
        if let Some(version) = payload.get("version").and_then(Value::as_str) {
            sanitized.insert("version".to_string(), Value::String(version.to_string()));
        }
        return;
    };
    if routes_to_upstream(root_name, upstream_name, destinations) {
        sanitized.insert("name".to_string(), Value::String(root_name.to_string()));
        if let Some(version) = payload.get("version").and_then(Value::as_str) {
            sanitized.insert("version".to_string(), Value::String(version.to_string()));
        }
    } else {
        sanitized.insert("name".to_string(), Value::String(REDACTED_ROOT_NAME.to_string()));
        if payload.contains_key("version") {
            sanitized.insert("version".to_string(), Value::String("0.0.0".to_string()));
        }
    }
}

fn copy_sanitized_arrays_and_metadata(
    payload: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
    sanitized: &mut Map<String, Value>,
) {
    for array_key in ["install", "remove"] {
        if let Some(items) = payload.get(array_key).and_then(Value::as_array) {
            sanitized.insert(
                array_key.to_string(),
                Value::Array(filter_spec_array(items, upstream_name, destinations)),
            );
        }
    }
    if let Some(metadata @ Value::Object(_)) = payload.get("metadata") {
        sanitized.insert("metadata".to_string(), metadata.clone());
    }
}

fn backfill_promoted_requires(requires: &mut Map<String, Value>, deps: &Map<String, Value>) {
    for dep_key in deps.keys() {
        if !requires.contains_key(dep_key) {
            requires.insert(dep_key.clone(), Value::String("*".to_string()));
        }
    }
}

fn filter_dependencies_map(
    deps: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
    depth: usize,
) -> Map<String, Value> {
    if depth > MAX_DEPENDENCY_DEPTH {
        return Map::new();
    }
    let mut kept = Map::new();
    let mut promoted = Map::new();
    for (package_name, value) in deps {
        let Some(entry) = value.as_object() else { continue };
        let nested_filtered = entry
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|nested| filter_dependencies_map(nested, upstream_name, destinations, depth + 1));

        if dependency_node_routes_to(package_name, entry, upstream_name, destinations) {
            kept.insert(
                package_name.clone(),
                Value::Object(sanitize_dependency_node(
                    entry,
                    nested_filtered,
                    upstream_name,
                    destinations,
                )),
            );
        } else {
            promote_nested_children(nested_filtered, &mut promoted);
        }
    }
    promote_nested_children(Some(promoted), &mut kept);
    kept
}

fn promote_nested_children(children: Option<Map<String, Value>>, target: &mut Map<String, Value>) {
    let Some(children) = children else { return };
    for (child_name, child_val) in children {
        target.entry(child_name).or_insert(child_val);
    }
}

fn dependency_node_routes_to(
    package_name: &str,
    entry: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> bool {
    if !routes_to_upstream(package_name, upstream_name, destinations) {
        return false;
    }
    if let Some(raw_version) = entry.get("version").and_then(Value::as_str)
        && let Some((alias_pkg, _)) = parse_npm_alias(raw_version)
    {
        return routes_to_upstream(alias_pkg, upstream_name, destinations);
    }
    true
}

fn sanitize_dependency_node(
    entry: &Map<String, Value>,
    nested_filtered: Option<Map<String, Value>>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut clean = Map::new();
    copy_string_fields(entry, &["version", "integrity"], &mut clean);
    copy_bool_fields(entry, &["dev", "optional", "bundled"], &mut clean);
    copy_filtered_spec_fields(entry, &["requires"], upstream_name, destinations, &mut clean);
    if let Some(children) = nested_filtered
        && !children.is_empty()
    {
        clean.insert("dependencies".to_string(), Value::Object(children));
    }
    clean
}

fn filter_packages_map(
    packages: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut kept = Map::new();
    if let Some(root_entry) = packages.get("").and_then(Value::as_object) {
        kept.insert(
            String::new(),
            Value::Object(sanitize_root_package_entry(root_entry, upstream_name, destinations)),
        );
    }
    for (key, value) in packages {
        if key.is_empty() {
            continue;
        }
        let Some(entry) = value.as_object() else { continue };
        let Some((sanitized_key, sanitized_entry)) =
            sanitize_installed_package_entry(key, entry, upstream_name, destinations)
        else {
            continue;
        };
        kept.entry(sanitized_key)
            .or_insert(Value::Object(sanitized_entry));
    }
    kept
}

fn sanitize_root_package_entry(
    entry: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut clean = Map::new();
    copy_sanitized_root_identity(entry, upstream_name, destinations, &mut clean);
    copy_filtered_spec_fields(entry, &PACKAGE_SPEC_FIELDS, upstream_name, destinations, &mut clean);
    clean
}

fn sanitize_installed_package_entry(
    key: &str,
    entry: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Option<(String, Map<String, Value>)> {
    if entry_bool(entry, "link") {
        return None;
    }
    let segments = node_modules_segments(key);
    let last_segment = *segments.last()?;
    if !routes_to_upstream(last_segment, upstream_name, destinations) {
        return None;
    }
    if let Some(explicit_name) = entry.get("name").and_then(Value::as_str)
        && !routes_to_upstream(explicit_name, upstream_name, destinations)
    {
        return None;
    }
    if let Some(raw_version) = entry.get("version").and_then(Value::as_str)
        && let Some((alias_pkg, _)) = parse_npm_alias(raw_version)
        && !routes_to_upstream(alias_pkg, upstream_name, destinations)
    {
        return None;
    }
    let kept_segments: Vec<&str> = segments
        .into_iter()
        .filter(|segment| routes_to_upstream(segment, upstream_name, destinations))
        .collect();
    if kept_segments.is_empty() {
        return None;
    }
    let sanitized_key = format!("node_modules/{}", kept_segments.join("/node_modules/"));
    Some((sanitized_key, build_clean_package_node(entry, upstream_name, destinations)))
}

fn build_clean_package_node(
    entry: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut clean = Map::new();
    copy_string_fields(entry, &["name", "version", "integrity"], &mut clean);
    copy_bool_fields(
        entry,
        &["dev", "optional", "devOptional", "inBundle", "hasInstallScript", "peer"],
        &mut clean,
    );
    copy_filtered_spec_fields(entry, &PACKAGE_SPEC_FIELDS, upstream_name, destinations, &mut clean);
    clean
}

fn copy_string_fields(entry: &Map<String, Value>, keys: &[&str], out: &mut Map<String, Value>) {
    for &key in keys {
        if let Some(val @ Value::String(_)) = entry.get(key) {
            out.insert(key.to_string(), val.clone());
        }
    }
}

fn copy_bool_fields(entry: &Map<String, Value>, keys: &[&str], out: &mut Map<String, Value>) {
    for &key in keys {
        if let Some(val @ Value::Bool(_)) = entry.get(key) {
            out.insert(key.to_string(), val.clone());
        }
    }
}

fn copy_filtered_spec_fields(
    entry: &Map<String, Value>,
    keys: &[&str],
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
    out: &mut Map<String, Value>,
) {
    for &field in keys {
        if let Some(spec_map) = entry.get(field).and_then(Value::as_object) {
            let filtered = filter_spec_map(spec_map, upstream_name, destinations);
            if !filtered.is_empty() {
                out.insert(field.to_string(), Value::Object(filtered));
            }
        }
    }
}

fn filter_spec_map(
    spec_map: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let mut clean = Map::new();
    for (package_name, spec_val) in spec_map {
        if !routes_to_upstream(package_name, upstream_name, destinations) {
            continue;
        }
        let Some(spec_str) = spec_val.as_str() else { continue };
        if is_non_registry_spec(spec_str) {
            continue;
        }
        if let Some((alias_pkg, _)) = parse_npm_alias(spec_str)
            && !routes_to_upstream(alias_pkg, upstream_name, destinations)
        {
            continue;
        }
        clean.insert(package_name.clone(), Value::String(spec_str.to_string()));
    }
    clean
}

fn filter_spec_array(
    items: &[Value],
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Vec<Value> {
    items
        .iter()
        .filter_map(Value::as_str)
        .filter(|raw| !is_non_registry_spec(raw))
        .filter(|raw| {
            parse_name_and_spec(raw)
                .is_some_and(|(package_name, _)| {
                    routes_to_upstream(package_name, upstream_name, destinations)
                })
        })
        .map(|raw| Value::String(raw.to_string()))
        .collect()
}

fn is_non_registry_spec(spec: &str) -> bool {
    let trimmed = spec.trim();
    trimmed.contains("://")
        || trimmed.starts_with("file:")
        || trimmed.starts_with("link:")
        || trimmed.starts_with("workspace:")
        || trimmed.starts_with("git+")
        || trimmed.starts_with("github:")
}

fn routes_to_upstream(
    package_name: &str,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> bool {
    match destinations.get(package_name) {
        Some(PackageDestination::Upstream(owner)) => owner == upstream_name,
        Some(PackageDestination::Hosted | PackageDestination::Skip) | None => false,
    }
}
