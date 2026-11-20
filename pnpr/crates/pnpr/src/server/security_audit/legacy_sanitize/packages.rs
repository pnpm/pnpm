use std::collections::{
    HashMap,
    HashSet,
};

use serde_json::{
    Map,
    Value,
};

use super::{
    LocalScopePool,
    PACKAGE_SPEC_FIELDS,
    REDACTED_SCOPE_PREFIX,
    SanitizeCtx,
    collect_reserved_scope_indices,
    copy_bool_fields,
    copy_sri_integrity,
    entry_bool,
    is_safe_semver_version,
    node_modules_segments,
    parse_npm_alias,
};

struct ResolvedPackageCoordinate {
    final_segment: String,
    clean_version: String,
    clean_name: Option<String>,
}

impl SanitizeCtx<'_> {
    pub(super) fn filter_packages_map(
        &mut self,
        packages: &Map<String, Value>,
    ) -> Map<String, Value> {
        let mut kept = Map::new();
        if let Some(root_entry) = packages.get("").and_then(Value::as_object) {
            kept.insert(String::new(), Value::Object(self.sanitize_root_package_entry(root_entry)));
        }
        let mut rewritten = Vec::new();
        for (key, value) in packages {
            if key.is_empty() {
                continue;
            }
            let Some(entry) = value.as_object() else { continue };
            let Some((sanitized_key, sanitized_entry)) =
                self.sanitize_installed_package_entry(key, entry)
            else {
                continue;
            };
            let entry_val = Value::Object(sanitized_entry);
            if sanitized_key == *key {
                kept.insert(sanitized_key, entry_val);
            } else {
                rewritten.push((sanitized_key, entry_val));
            }
        }
        self.insert_rewritten_packages(&mut kept, rewritten);
        kept
    }

    fn insert_rewritten_packages(
        &mut self,
        kept: &mut Map<String, Value>,
        rewritten: Vec<(String, Value)>,
    ) {
        if rewritten.is_empty() {
            return;
        }
        let mut local_pool: Option<LocalScopePool> = None;
        let mut next_ordinal_by_key: HashMap<String, usize> = HashMap::new();
        for (sanitized_key, entry_val) in rewritten {
            if !kept.contains_key(&sanitized_key) {
                kept.insert(sanitized_key, entry_val);
                continue;
            }
            if kept.get(&sanitized_key) == Some(&entry_val) {
                continue;
            }
            let pool = local_pool.get_or_insert_with(|| init_local_pkg_pool(kept));
            let ordinal = next_ordinal_by_key.entry(sanitized_key.clone()).or_insert(0);
            let distinct_key =
                self.allocate_pooled_package_key(kept, pool, &sanitized_key, ordinal);
            kept.insert(distinct_key, entry_val);
        }
    }

    fn allocate_pooled_package_key(
        &mut self,
        kept: &Map<String, Value>,
        pool: &mut LocalScopePool,
        sanitized_key: &str,
        ordinal: &mut usize,
    ) -> String {
        loop {
            let idx = pool.scope_index_at(*ordinal, |ord| self.global_scope_index_at(ord));
            *ordinal += 1;
            let scope_name = format!("{REDACTED_SCOPE_PREFIX}{idx}");
            let candidate = format!("node_modules/{scope_name}/{sanitized_key}");
            if !kept.contains_key(&candidate) {
                return candidate;
            }
        }
    }

    fn sanitize_root_package_entry(&self, entry: &Map<String, Value>) -> Map<String, Value> {
        let mut clean = Map::new();
        self.copy_sanitized_root_identity(entry, &mut clean);
        self.copy_filtered_spec_fields(entry, &PACKAGE_SPEC_FIELDS, &mut clean);
        clean
    }

    fn sanitize_installed_package_entry(
        &self,
        key: &str,
        entry: &Map<String, Value>,
    ) -> Option<(String, Map<String, Value>)> {
        if entry_bool(entry, "link") {
            return None;
        }
        let segments = node_modules_segments(key);
        let &last_segment = segments.last()?;
        let coord = self.resolve_package_coordinate(last_segment, entry)?;
        let kept_parents: Vec<&str> = segments[..segments.len() - 1]
            .iter()
            .copied()
            .filter(|segment| self.routes(segment))
            .collect();
        let sanitized_key = if kept_parents.is_empty() {
            format!("node_modules/{}", coord.final_segment)
        } else {
            format!(
                "node_modules/{}/node_modules/{}",
                kept_parents.join("/node_modules/"),
                coord.final_segment,
            )
        };
        let node = self.build_clean_package_node(entry, coord);
        Some((sanitized_key, node))
    }

    fn resolve_package_coordinate(
        &self,
        last_segment: &str,
        entry: &Map<String, Value>,
    ) -> Option<ResolvedPackageCoordinate> {
        let explicit_name = entry.get("name").and_then(Value::as_str);
        let declared_name = explicit_name.unwrap_or(last_segment);
        let raw_version = entry.get("version").and_then(Value::as_str)?;
        if let Some(alias) = parse_npm_alias(raw_version) {
            return self.resolve_aliased_package_coordinate(
                last_segment,
                explicit_name,
                raw_version,
                alias,
            );
        }
        if !is_safe_semver_version(raw_version) || !self.routes(declared_name) {
            return None;
        }
        let final_segment = if self.routes(last_segment) { last_segment } else { declared_name };
        let clean_name = explicit_name
            .filter(|&name| name == final_segment || self.routes(name))
            .map(str::to_owned);
        Some(ResolvedPackageCoordinate {
            final_segment: final_segment.to_string(),
            clean_version: raw_version.to_string(),
            clean_name,
        })
    }

    fn resolve_aliased_package_coordinate(
        &self,
        last_segment: &str,
        explicit_name: Option<&str>,
        raw_version: &str,
        (alias_pkg, alias_ver_opt): (&str, Option<&str>),
    ) -> Option<ResolvedPackageCoordinate> {
        let declared_name = explicit_name.unwrap_or(last_segment);
        let alias_ver = alias_ver_opt.filter(|ver| is_safe_semver_version(ver))?;
        if !self.routes(alias_pkg) {
            return None;
        }
        if self.routes(last_segment) && self.routes(declared_name) {
            return Some(ResolvedPackageCoordinate {
                final_segment: last_segment.to_string(),
                clean_version: raw_version.to_string(),
                clean_name: explicit_name.map(str::to_owned),
            });
        }
        Some(ResolvedPackageCoordinate {
            final_segment: alias_pkg.to_string(),
            clean_version: alias_ver.to_string(),
            clean_name: None,
        })
    }

    fn build_clean_package_node(
        &self,
        entry: &Map<String, Value>,
        coord: ResolvedPackageCoordinate,
    ) -> Map<String, Value> {
        let mut clean = Map::new();
        if let Some(name) = coord.clean_name {
            clean.insert("name".to_string(), Value::String(name));
        }
        clean.insert("version".to_string(), Value::String(coord.clean_version));
        copy_sri_integrity(entry, &mut clean);
        copy_bool_fields(
            entry,
            &["dev", "optional", "devOptional", "inBundle", "hasInstallScript", "peer"],
            &mut clean,
        );
        self.copy_filtered_spec_fields(entry, &PACKAGE_SPEC_FIELDS, &mut clean);
        clean
    }
}

fn init_local_pkg_pool(kept: &Map<String, Value>) -> LocalScopePool {
    let mut local_reserved = HashSet::new();
    collect_reserved_scope_indices(
        kept.keys().flat_map(|key| node_modules_segments(key)),
        &mut local_reserved,
    );
    LocalScopePool::new(local_reserved)
}
