use std::collections::{HashMap, HashSet};

use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use serde_json::{Map, Value};

use super::{
    Action, AppState, PackageDestination, RegistrySource, authorize, classify_package,
    legacy_report::InstalledNode,
    legacy_specs::{entry_bool, node_modules_segments, parse_name_and_spec, parse_npm_alias},
    resolve_registry_source,
};

pub(super) const MAX_DEPENDENCY_DEPTH: usize = 64;

pub(super) const PACKAGE_SPEC_FIELDS: [&str; 5] =
    ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies", "requires"];

pub(super) struct LegacyClassification {
    pub(super) destinations: HashMap<String, PackageDestination>,
    pub(super) referenced_upstreams: HashSet<String>,
}

#[derive(Clone, Copy)]
struct AncestorFlags {
    is_dev: bool,
    is_optional: bool,
    is_bundled: bool,
}

impl AncestorFlags {
    fn inherit(self, entry: &Map<String, Value>) -> Self {
        Self {
            is_dev: self.is_dev || entry_bool(entry, "dev"),
            is_optional: self.is_optional || entry_bool(entry, "optional"),
            is_bundled: self.is_bundled || entry_bool(entry, "bundled"),
        }
    }
}

#[derive(Default)]
struct CollectedLegacyNames {
    required: HashSet<String>,
    non_failing_labels: HashSet<String>,
}

impl CollectedLegacyNames {
    fn record_spec(&mut self, outer_name: &str, raw_spec: Option<&str>) {
        if let Some(spec_str) = raw_spec
            && let Some((alias_pkg, _)) = parse_npm_alias(spec_str)
        {
            self.required.insert(alias_pkg.to_string());
            self.non_failing_labels.insert(outer_name.to_string());
        } else {
            self.required.insert(outer_name.to_string());
        }
    }
}

pub(super) fn classify_legacy_packages(
    state: &AppState,
    identity: &Identity,
    target: &str,
    payload: &Map<String, Value>,
    nodes: &[InstalledNode],
) -> Result<LegacyClassification, RegistryError> {
    let mut collected = CollectedLegacyNames::default();
    for node in nodes {
        collected.required.insert(node.package.clone());
    }
    collect_dependency_package_names(payload, &mut collected);

    let mut destinations =
        HashMap::with_capacity(collected.required.len() + collected.non_failing_labels.len() + 2);
    let mut referenced_upstreams = HashSet::new();
    for package_name in collected.required {
        let destination = classify_package(state, identity, target, &package_name)?;
        if let PackageDestination::Upstream(ref upstream_name) = destination {
            referenced_upstreams.insert(upstream_name.clone());
        }
        destinations.insert(package_name, destination);
    }
    classify_non_failing_labels(
        state,
        identity,
        target,
        payload,
        collected.non_failing_labels,
        &mut destinations,
    );
    Ok(LegacyClassification { destinations, referenced_upstreams })
}

fn classify_non_failing_labels(
    state: &AppState,
    identity: &Identity,
    target: &str,
    payload: &Map<String, Value>,
    mut labels: HashSet<String>,
    destinations: &mut HashMap<String, PackageDestination>,
) {
    if let Some(name) = payload.get("name").and_then(Value::as_str) {
        labels.insert(name.to_string());
    }
    if let Some(root_pkg_name) = payload
        .get("packages")
        .and_then(Value::as_object)
        .and_then(|pkgs| pkgs.get(""))
        .and_then(|root| root.get("name"))
        .and_then(Value::as_str)
    {
        labels.insert(root_pkg_name.to_string());
    }
    for candidate in labels {
        if destinations.contains_key(&candidate) {
            continue;
        }
        let destination = classify_non_failing_root_name(state, identity, target, &candidate);
        destinations.insert(candidate, destination);
    }
}

fn classify_non_failing_root_name(
    state: &AppState,
    identity: &Identity,
    target: &str,
    raw_name: &str,
) -> PackageDestination {
    let Ok(canonical) = CanonicalPackageName::parse(raw_name, Ecosystem::Npm) else {
        return PackageDestination::Skip;
    };
    let resolved = resolve_registry_source(state, target, canonical.as_str());
    match &resolved {
        RegistrySource::Upstream(source)
            if authorize(state, identity, &resolved, canonical.as_str(), Action::Read).is_ok() =>
        {
            PackageDestination::Upstream(source.clone())
        }
        RegistrySource::Hosted(_) => PackageDestination::Hosted,
        RegistrySource::Upstream(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => {
            PackageDestination::Skip
        }
    }
}

fn collect_dependency_package_names(
    payload: &Map<String, Value>,
    collected: &mut CollectedLegacyNames,
) {
    if let Some(requires) = payload.get("requires").and_then(Value::as_object) {
        collect_spec_map_names(requires, collected);
    }
    if let Some(deps) = payload.get("dependencies").and_then(Value::as_object) {
        collect_dependencies_names(deps, collected, 0);
    }
    if let Some(packages) = payload.get("packages").and_then(Value::as_object) {
        collect_packages_map_names(packages, collected);
    }
    for array_key in ["install", "remove"] {
        if let Some(items) = payload.get(array_key).and_then(Value::as_array) {
            collect_spec_array_names(items, collected);
        }
    }
}

fn collect_dependencies_names(
    deps: &Map<String, Value>,
    collected: &mut CollectedLegacyNames,
    depth: usize,
) {
    if depth > MAX_DEPENDENCY_DEPTH {
        return;
    }
    for (package_name, value) in deps {
        if let Some(node) = value.as_object() {
            collect_dependency_object_names(package_name, node, collected, depth);
        } else {
            collected.required.insert(package_name.clone());
        }
    }
}

fn collect_dependency_object_names(
    package_name: &str,
    node: &Map<String, Value>,
    collected: &mut CollectedLegacyNames,
    depth: usize,
) {
    collected.record_spec(package_name, node.get("version").and_then(Value::as_str));
    if let Some(requires) = node.get("requires").and_then(Value::as_object) {
        collect_spec_map_names(requires, collected);
    }
    if let Some(nested) = node.get("dependencies").and_then(Value::as_object) {
        collect_dependencies_names(nested, collected, depth + 1);
    }
}

fn collect_packages_map_names(packages: &Map<String, Value>, collected: &mut CollectedLegacyNames) {
    for (key, value) in packages {
        if let Some(entry) = value.as_object() {
            collect_package_entry_names(key, entry, collected);
        }
    }
}

fn collect_package_entry_names(
    key: &str,
    entry: &Map<String, Value>,
    collected: &mut CollectedLegacyNames,
) {
    for field in PACKAGE_SPEC_FIELDS {
        if let Some(spec_map) = entry.get(field).and_then(Value::as_object) {
            collect_spec_map_names(spec_map, collected);
        }
    }
    if key.is_empty() || entry_bool(entry, "link") {
        return;
    }
    record_installed_package_entry(key, entry, collected);
}

fn record_installed_package_entry(
    key: &str,
    entry: &Map<String, Value>,
    collected: &mut CollectedLegacyNames,
) {
    let segments = node_modules_segments(key);
    let Some(&last_segment) = segments.last() else { return };
    for &parent_segment in &segments[..segments.len() - 1] {
        collected.non_failing_labels.insert(parent_segment.to_string());
    }
    let explicit_name = entry.get("name").and_then(Value::as_str);
    let declared_name = explicit_name.unwrap_or(last_segment);
    let raw_version = entry.get("version").and_then(Value::as_str);
    collected.record_spec(declared_name, raw_version);
    if declared_name != last_segment {
        collected.non_failing_labels.insert(last_segment.to_string());
    }
}

fn collect_spec_map_names(spec_map: &Map<String, Value>, collected: &mut CollectedLegacyNames) {
    for (package_name, spec_val) in spec_map {
        collected.record_spec(package_name, spec_val.as_str());
    }
}

fn collect_spec_array_names(items: &[Value], collected: &mut CollectedLegacyNames) {
    for raw in items.iter().filter_map(Value::as_str) {
        if let Some((package_name, spec)) = parse_name_and_spec(raw) {
            collected.record_spec(package_name, spec);
        }
    }
}

pub(super) fn collect_installed_nodes(
    payload: &Map<String, Value>,
) -> Result<Vec<InstalledNode>, ()> {
    let mut nodes = Vec::new();
    if let Some(deps) = payload.get("dependencies").and_then(Value::as_object) {
        let root_flags = AncestorFlags { is_dev: false, is_optional: false, is_bundled: false };
        collect_from_dependencies(deps, "", root_flags, 0, &mut nodes)?;
        return Ok(nodes);
    }
    if let Some(packages) = payload.get("packages").and_then(Value::as_object) {
        collect_from_packages(packages, &mut nodes)?;
        return Ok(nodes);
    }
    if let Some(requires) = payload.get("requires").and_then(Value::as_object) {
        collect_from_requires(requires, &mut nodes);
    }
    Ok(nodes)
}

fn collect_from_dependencies(
    deps: &Map<String, Value>,
    parent_path: &str,
    inherited: AncestorFlags,
    depth: usize,
    nodes: &mut Vec<InstalledNode>,
) -> Result<(), ()> {
    if depth > MAX_DEPENDENCY_DEPTH {
        return Err(());
    }
    for (key, value) in deps {
        let Some(entry) = value.as_object() else { continue };
        let path = join_dependency_path(parent_path, key);
        let flags = inherited.inherit(entry);
        if let Some((package, version)) = resolve_dependency_coordinate(key, entry) {
            nodes.push(InstalledNode {
                package,
                version,
                path: path.clone(),
                is_dev: flags.is_dev,
                is_optional: flags.is_optional,
                is_bundled: flags.is_bundled,
            });
        }
        if let Some(nested) = entry.get("dependencies").and_then(Value::as_object) {
            collect_from_dependencies(nested, &path, flags, depth + 1, nodes)?;
        }
    }
    Ok(())
}

fn join_dependency_path(parent_path: &str, key: &str) -> String {
    if parent_path.is_empty() { key.to_string() } else { format!("{parent_path}>{key}") }
}

fn resolve_dependency_coordinate(
    key: &str,
    entry: &Map<String, Value>,
) -> Option<(String, String)> {
    let raw_version = entry.get("version").and_then(Value::as_str)?;
    if let Some((alias_pkg, Some(alias_ver))) = parse_npm_alias(raw_version)
        && !alias_ver.is_empty()
    {
        return Some((alias_pkg.to_string(), alias_ver.to_string()));
    }
    (!raw_version.is_empty()).then(|| (key.to_string(), raw_version.to_string()))
}

fn collect_from_packages(
    packages: &Map<String, Value>,
    nodes: &mut Vec<InstalledNode>,
) -> Result<(), ()> {
    for (key, value) in packages {
        if let Some(node) = parse_installed_package_node(key, value)? {
            nodes.push(node);
        }
    }
    Ok(())
}

fn parse_installed_package_node(key: &str, value: &Value) -> Result<Option<InstalledNode>, ()> {
    if key.is_empty() {
        return Ok(None);
    }
    let Some(entry) = value.as_object() else { return Ok(None) };
    if entry_bool(entry, "link") {
        return Ok(None);
    }
    let segments = node_modules_segments(key);
    if segments.len() > MAX_DEPENDENCY_DEPTH {
        return Err(());
    }
    let Some(&last_segment) = segments.last() else { return Ok(None) };
    let default_name = entry
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(last_segment);
    let Some((package, version)) = resolve_dependency_coordinate(default_name, entry) else {
        return Ok(None);
    };
    let flags = package_entry_flags(entry);
    Ok(Some(InstalledNode {
        package,
        version,
        path: segments.join(">"),
        is_dev: flags.is_dev,
        is_optional: flags.is_optional,
        is_bundled: flags.is_bundled,
    }))
}

fn package_entry_flags(entry: &Map<String, Value>) -> AncestorFlags {
    let dev_optional = entry_bool(entry, "devOptional");
    AncestorFlags {
        is_dev: dev_optional || entry_bool(entry, "dev"),
        is_optional: dev_optional || entry_bool(entry, "optional"),
        is_bundled: entry_bool(entry, "inBundle") || entry_bool(entry, "bundled"),
    }
}

fn collect_from_requires(requires: &Map<String, Value>, nodes: &mut Vec<InstalledNode>) {
    for (key, value) in requires {
        let Some(raw_spec) = value.as_str() else { continue };
        let (package, candidate) = match parse_npm_alias(raw_spec) {
            Some((alias_pkg, Some(alias_ver))) => (alias_pkg, alias_ver),
            Some((_, None)) => continue,
            None => (key.as_str(), raw_spec),
        };
        if node_semver::Version::parse(candidate).is_ok() {
            nodes.push(InstalledNode {
                package: package.to_string(),
                version: candidate.to_string(),
                path: key.clone(),
                is_dev: false,
                is_optional: false,
                is_bundled: false,
            });
        }
    }
}
