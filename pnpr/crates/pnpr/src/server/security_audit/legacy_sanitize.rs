mod dependencies;
mod packages;

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::{
    PackageDestination,
    legacy_nodes::{MAX_DEPENDENCY_DEPTH, PACKAGE_SPEC_FIELDS},
    legacy_specs::{
        REDACTED_SCOPE_PREFIX, entry_bool, is_safe_semver_range, is_safe_semver_version,
        is_safe_sri_integrity, node_modules_segments, parse_name_and_spec, parse_npm_alias,
    },
};

const REDACTED_ROOT_NAME: &str = "pnpr-audit";
const ALLOWED_PLATFORMS: [&str; 8] =
    ["linux", "darwin", "win32", "freebsd", "openbsd", "sunos", "aix", "android"];
const ALLOWED_NODE_ENVS: [&str; 3] = ["production", "development", "test"];

pub(super) struct SanitizeCtx<'a> {
    pub(super) upstream_name: &'a str,
    pub(super) destinations: &'a HashMap<String, PackageDestination>,
}

enum SanitizedSpecEntry {
    Direct { name: String, spec: Value },
    RewrittenAlias { target_name: String, range: Value },
}

pub(super) fn build_upstream_legacy_payload(
    payload: &Map<String, Value>,
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> Map<String, Value> {
    let ctx = SanitizeCtx { upstream_name, destinations };
    let mut sanitized = Map::new();
    ctx.copy_sanitized_root_identity(payload, &mut sanitized);
    if let Some(lockfile_version) = payload
        .get("lockfileVersion")
        .and_then(Value::as_u64)
        .filter(|version| (1..=3).contains(version))
    {
        sanitized.insert("lockfileVersion".to_string(), Value::from(lockfile_version));
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
            let Some(outcome) = self.sanitize_spec_map_entry(package_name, spec_str) else {
                continue;
            };
            match outcome {
                SanitizedSpecEntry::Direct { name, spec } => {
                    clean.insert(name, spec);
                }
                SanitizedSpecEntry::RewrittenAlias { target_name, range } => {
                    aliased.push((target_name, range));
                }
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
    ) -> Option<SanitizedSpecEntry> {
        if let Some((alias_pkg, alias_range)) = parse_npm_alias(spec_str) {
            let range_str = alias_range
                .filter(|range| !range.is_empty())
                .unwrap_or("*");
            if !self.routes(alias_pkg) || !is_safe_semver_range(range_str) {
                return None;
            }
            if self.routes(package_name) {
                return Some(SanitizedSpecEntry::Direct {
                    name: package_name.to_string(),
                    spec: Value::String(spec_str.to_string()),
                });
            }
            return Some(SanitizedSpecEntry::RewrittenAlias {
                target_name: alias_pkg.to_string(),
                range: Value::String(range_str.to_string()),
            });
        }
        if !self.routes(package_name) || !is_safe_semver_range(spec_str) {
            return None;
        }
        Some(SanitizedSpecEntry::Direct {
            name: package_name.to_string(),
            spec: Value::String(spec_str.to_string()),
        })
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
    if let Some(npm_ver) = raw
        .get("npm_version")
        .and_then(Value::as_str)
        .filter(|ver| is_safe_semver_version(ver))
    {
        clean.insert("npm_version".to_string(), Value::String(npm_ver.trim().to_string()));
    }
    if let Some(node_ver) = raw
        .get("node_version")
        .and_then(Value::as_str)
        .filter(|ver| {
            is_safe_semver_version(
                ver.trim()
                    .strip_prefix('v')
                    .unwrap_or(ver),
            )
        })
    {
        clean.insert("node_version".to_string(), Value::String(node_ver.trim().to_string()));
    }
    if let Some(platform) = raw
        .get("platform")
        .and_then(Value::as_str)
        .filter(|plat| ALLOWED_PLATFORMS.contains(plat))
    {
        clean.insert("platform".to_string(), Value::String(platform.to_string()));
    }
    if let Some(node_env) = raw
        .get("node_env")
        .and_then(Value::as_str)
        .filter(|env| ALLOWED_NODE_ENVS.contains(env))
    {
        clean.insert("node_env".to_string(), Value::String(node_env.to_string()));
    }
    (!clean.is_empty()).then_some(clean)
}

pub(super) fn copy_sri_integrity(entry: &Map<String, Value>, out: &mut Map<String, Value>) {
    if let Some(integrity) = entry
        .get("integrity")
        .and_then(Value::as_str)
        .filter(|raw| is_safe_sri_integrity(raw))
    {
        out.insert("integrity".to_string(), Value::String(integrity.trim().to_string()));
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
