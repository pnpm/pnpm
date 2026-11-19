use serde_json::{Map, Value};

use super::{
    MAX_DEPENDENCY_DEPTH, REDACTED_SCOPE_PREFIX, SanitizeCtx, copy_bool_fields, copy_string_fields,
    is_safe_semver_version, parse_npm_alias,
};

struct RoutedDependencyNode {
    key: String,
    version: String,
    is_unchanged_key: bool,
}

#[derive(Default)]
struct DependencyBuckets {
    kept: Map<String, Value>,
    deferred: Vec<(String, Value)>,
}

impl SanitizeCtx<'_> {
    pub(super) fn filter_dependencies_map(
        &self,
        deps: &Map<String, Value>,
        depth: usize,
    ) -> Map<String, Value> {
        if depth > MAX_DEPENDENCY_DEPTH {
            return Map::new();
        }
        let mut buckets = DependencyBuckets::default();
        for (package_name, value) in deps {
            if let Some(entry) = value.as_object() {
                self.collect_sanitized_dependency(package_name, entry, depth, &mut buckets);
            }
        }
        self.insert_deferred_dependencies(&mut buckets.kept, buckets.deferred);
        buckets.kept
    }

    fn collect_sanitized_dependency(
        &self,
        package_name: &str,
        entry: &Map<String, Value>,
        depth: usize,
        buckets: &mut DependencyBuckets,
    ) {
        let nested_filtered = entry
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|nested| self.filter_dependencies_map(nested, depth + 1));

        let Some(routed) = self.resolve_routed_dependency_node(package_name, entry) else {
            if let Some(children) = nested_filtered {
                buckets.deferred.extend(children);
            }
            return;
        };
        let clean =
            Value::Object(self.sanitize_dependency_node(entry, routed.version, nested_filtered));
        if routed.is_unchanged_key {
            buckets.kept.insert(routed.key, clean);
        } else {
            buckets.deferred.push((routed.key, clean));
        }
    }

    fn resolve_routed_dependency_node(
        &self,
        package_name: &str,
        entry: &Map<String, Value>,
    ) -> Option<RoutedDependencyNode> {
        let raw_version = entry.get("version").and_then(Value::as_str)?;
        if let Some((alias_pkg, alias_ver_opt)) = parse_npm_alias(raw_version) {
            let alias_ver = alias_ver_opt.filter(|ver| is_safe_semver_version(ver))?;
            if !self.routes(alias_pkg) {
                return None;
            }
            if self.routes(package_name) {
                return Some(RoutedDependencyNode {
                    key: package_name.to_string(),
                    version: raw_version.to_string(),
                    is_unchanged_key: true,
                });
            }
            return Some(RoutedDependencyNode {
                key: alias_pkg.to_string(),
                version: alias_ver.to_string(),
                is_unchanged_key: false,
            });
        }
        if !self.routes(package_name) || !is_safe_semver_version(raw_version) {
            return None;
        }
        Some(RoutedDependencyNode {
            key: package_name.to_string(),
            version: raw_version.to_string(),
            is_unchanged_key: true,
        })
    }

    fn insert_deferred_dependencies(
        &self,
        kept: &mut Map<String, Value>,
        deferred: Vec<(String, Value)>,
    ) {
        for (child_name, child_val) in deferred {
            if !kept.contains_key(&child_name) {
                kept.insert(child_name, child_val);
                continue;
            }
            if kept.get(&child_name) == Some(&child_val) {
                continue;
            }
            self.insert_into_redacted_scope(kept, child_name, child_val);
        }
    }

    fn insert_into_redacted_scope(
        &self,
        kept: &mut Map<String, Value>,
        child_name: String,
        child_val: Value,
    ) {
        let mut idx = 1usize;
        loop {
            let scope_name = format!("{REDACTED_SCOPE_PREFIX}{idx}");
            if self.destinations.contains_key(&scope_name) {
                idx += 1;
                continue;
            }
            if !kept.contains_key(&scope_name) {
                kept.insert(scope_name, build_redacted_scope_node(child_name, child_val));
                return;
            }
            if try_insert_into_existing_scope(kept.get_mut(&scope_name), &child_name, &child_val) {
                return;
            }
            idx += 1;
        }
    }

    fn sanitize_dependency_node(
        &self,
        entry: &Map<String, Value>,
        version: String,
        nested_filtered: Option<Map<String, Value>>,
    ) -> Map<String, Value> {
        let mut clean = Map::new();
        clean.insert("version".to_string(), Value::String(version));
        copy_string_fields(entry, &["integrity"], &mut clean);
        copy_bool_fields(entry, &["dev", "optional", "bundled"], &mut clean);
        self.copy_filtered_spec_fields(entry, &["requires"], &mut clean);
        if let Some(children) = nested_filtered
            && !children.is_empty()
        {
            clean.insert("dependencies".to_string(), Value::Object(children));
        }
        clean
    }
}

fn build_redacted_scope_node(child_name: String, child_val: Value) -> Value {
    let mut deps = Map::new();
    deps.insert(child_name, child_val);
    let mut scope = Map::new();
    scope.insert("version".to_string(), Value::String("0.0.0".to_string()));
    scope.insert("dependencies".to_string(), Value::Object(deps));
    Value::Object(scope)
}

fn try_insert_into_existing_scope(
    scope_entry: Option<&mut Value>,
    child_name: &str,
    child_val: &Value,
) -> bool {
    let Some(deps) = scope_entry
        .and_then(Value::as_object_mut)
        .and_then(|obj| obj.get_mut("dependencies"))
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    if deps.get(child_name) == Some(child_val) {
        return true;
    }
    if deps.contains_key(child_name) {
        return false;
    }
    deps.insert(child_name.to_string(), child_val.clone());
    true
}
