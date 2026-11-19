mod dependencies;
mod packages;

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::{
    PackageDestination,
    legacy_nodes::{MAX_DEPENDENCY_DEPTH, PACKAGE_SPEC_FIELDS},
    legacy_specs::{
        REDACTED_SCOPE_PREFIX, entry_bool, is_safe_semver_range, is_safe_semver_version,
        node_modules_segments, parse_name_and_spec, parse_npm_alias,
    },
};

const REDACTED_ROOT_NAME: &str = "pnpr-audit";
const SAFE_CLIENT_METADATA_KEYS: [&str; 4] =
    ["npm_version", "node_version", "platform", "node_env"];

pub(super) struct SanitizeCtx<'a> {
    pub(super) upstream_name: &'a str,
    pub(super) destinations: &'a HashMap<String, PackageDestination>,
}

pub(super) fn build_upstream_legacy_payload(
    payload: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let ctx = SanitizeCtx { upstream_name, destinations };
    let mut sanitized = Map::new();
    ctx.copy_sanitized_root_identity(payload, &mut sanitized);
    if let Some(lockfile_version) = payload.get("lockfileVersion") {
        sanitized.insert("lockfileVersion".to_string(), lockfile_version.clone());
    }
    let mut filtered_requires = payload
        .get("requires")
        .and_then(Value::as_object)
        .map(|req| ctx.filter_spec_map(req));

    if let Some(deps) = payload.get("dependencies").and_then(Value::as_object) {
        let filtered_deps = ctx.filter_dependencies_map(deps, 0);
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
        sanitized.insert("packages".to_string(), Value::Object(ctx.filter_packages_map(packages)));
    }
    ctx.copy_sanitized_arrays_and_metadata(payload, &mut sanitized);
    sanitized
}

fn backfill_promoted_requires(requires: &mut Map<String, Value>, deps: &Map<String, Value>) {
    for dep_key in deps.keys() {
        if !requires.contains_key(dep_key) {
            requires.insert(dep_key.clone(), Value::String("*".to_string()));
        }
    }
}

impl SanitizeCtx<'_> {
    pub(super) fn routes(&self, package_name: &str) -> bool {
        match self.destinations.get(package_name) {
            Some(PackageDestination::Upstream(owner)) => owner == self.upstream_name,
            Some(PackageDestination::Hosted | PackageDestination::Skip) | None => false,
        }
    }

    pub(super) fn copy_sanitized_root_identity(
        &self,
        payload: &Map<String, Value>,
        sanitized: &mut Map<String, Value>,
    ) {
        let Some(root_name) = payload.get("name").and_then(Value::as_str) else {
            if let Some(version) = payload
                .get("version")
                .and_then(Value::as_str)
                .filter(|ver| is_safe_semver_version(ver))
            {
                sanitized.insert("version".to_string(), Value::String(version.to_string()));
            }
            return;
        };
        if self.routes(root_name) {
            sanitized.insert("name".to_string(), Value::String(root_name.to_string()));
            if let Some(version) = payload
                .get("version")
                .and_then(Value::as_str)
                .filter(|ver| is_safe_semver_version(ver))
            {
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
        &self,
        payload: &Map<String, Value>,
        sanitized: &mut Map<String, Value>,
    ) {
        for array_key in ["install", "remove"] {
            if let Some(items) = payload.get(array_key).and_then(Value::as_array) {
                sanitized.insert(
                    array_key.to_string(),
                    Value::Array(self.filter_spec_array(items)),
                );
            }
        }
        if let Some(raw_meta) = payload.get("metadata").and_then(Value::as_object)
            && let Some(clean_meta) = sanitize_client_metadata(raw_meta)
        {
            sanitized.insert("metadata".to_string(), Value::Object(clean_meta));
        }
    }

    pub(super) fn copy_filtered_spec_fields(
        &self,
        entry: &Map<String, Value>,
        keys: &[&str],
        out: &mut Map<String, Value>,
    ) {
        for &field in keys {
            if let Some(spec_map) = entry.get(field).and_then(Value::as_object) {
                let filtered = self.filter_spec_map(spec_map);
                if !filtered.is_empty() {
                    out.insert(field.to_string(), Value::Object(filtered));
                }
            }
        }
    }

    pub(super) fn filter_spec_map(&self, spec_map: &Map<String, Value>) -> Map<String, Value> {
        let mut clean = Map::new();
        let mut aliased = Vec::new();
        for (package_name, spec_val) in spec_map {
            let Some(spec_str) = spec_val.as_str() else { continue };
            let Some((key, val, is_direct)) = self.sanitize_spec_map_entry(package_name, spec_str)
            else {
                continue;
            };
            if is_direct {
                clean.insert(key, val);
            } else {
                aliased.push((key, val));
            }
        }
        for (alias_pkg, range_val) in aliased {
            clean.entry(alias_pkg).or_insert(range_val);
        }
        clean
    }

    fn sanitize_spec_map_entry(
        &self,
        package_name: &str,
        spec_str: &str,
    ) -> Option<(String, Value, bool)> {
        if let Some((alias_pkg, alias_range)) = parse_npm_alias(spec_str) {
            let range_str = alias_range
                .filter(|range| !range.is_empty())
                .unwrap_or("*");
            if !self.routes(alias_pkg) || !is_safe_semver_range(range_str) {
                return None;
            }
            if self.routes(package_name) {
                return Some((package_name.to_string(), Value::String(spec_str.to_string()), true));
            }
            return Some((alias_pkg.to_string(), Value::String(range_str.to_string()), false));
        }
        if !self.routes(package_name) || !is_safe_semver_range(spec_str) {
            return None;
        }
        Some((package_name.to_string(), Value::String(spec_str.to_string()), true))
    }

    fn filter_spec_array(&self, items: &[Value]) -> Vec<Value> {
        items
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|raw| self.sanitize_spec_array_item(raw))
            .map(Value::String)
            .collect()
    }

    fn sanitize_spec_array_item(&self, raw: &str) -> Option<String> {
        let (package_name, spec_opt) = parse_name_and_spec(raw)?;
        let Some(spec_str) = spec_opt else {
            return self.routes(package_name).then(|| raw.to_string());
        };
        if let Some((alias_pkg, alias_ver_opt)) = parse_npm_alias(spec_str) {
            return self.sanitize_aliased_array_item(raw, package_name, alias_pkg, alias_ver_opt);
        }
        if !self.routes(package_name) {
            return None;
        }
        if !spec_str.is_empty() && !is_safe_semver_range(spec_str) {
            return None;
        }
        Some(raw.to_string())
    }

    fn sanitize_aliased_array_item(
        &self,
        raw: &str,
        package_name: &str,
        alias_pkg: &str,
        alias_ver_opt: Option<&str>,
    ) -> Option<String> {
        if !self.routes(alias_pkg) {
            return None;
        }
        let clean_ver = alias_ver_opt.filter(|ver| !ver.is_empty());
        if clean_ver.is_some_and(|ver| !is_safe_semver_range(ver)) {
            return None;
        }
        if self.routes(package_name) {
            return Some(raw.to_string());
        }
        Some(match clean_ver {
            Some(ver) => format!("{alias_pkg}@{ver}"),
            None => alias_pkg.to_string(),
        })
    }
}

fn sanitize_client_metadata(raw: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut clean = Map::new();
    for key in SAFE_CLIENT_METADATA_KEYS {
        if let Some(val) = raw
            .get(key)
            .and_then(Value::as_str)
            .filter(|scalar| is_safe_metadata_scalar(scalar))
        {
            clean.insert(key.to_string(), Value::String(val.to_string()));
        }
    }
    (!clean.is_empty()).then_some(clean)
}

fn is_safe_metadata_scalar(val: &str) -> bool {
    !val.is_empty()
        && val.len() <= 128
        && !val.contains('/')
        && !val.contains(':')
        && !val.contains('@')
}

pub(super) fn copy_string_fields(
    entry: &Map<String, Value>,
    keys: &[&str],
    out: &mut Map<String, Value>,
) {
    for &key in keys {
        if let Some(val @ Value::String(_)) = entry.get(key) {
            out.insert(key.to_string(), val.clone());
        }
    }
}

pub(super) fn copy_bool_fields(
    entry: &Map<String, Value>,
    keys: &[&str],
    out: &mut Map<String, Value>,
) {
    for &key in keys {
        if let Some(val @ Value::Bool(_)) = entry.get(key) {
            out.insert(key.to_string(), val.clone());
        }
    }
}
