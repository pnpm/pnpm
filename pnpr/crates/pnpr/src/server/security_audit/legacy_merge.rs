use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::legacy_report::{LegacyAuditMetadata, LegacyVulnerabilityCounts};

pub(super) fn sanitize_upstream_legacy_response(
    doc: &mut Map<String, Value>,
    allowed_packages: &HashSet<String>,
) {
    if let Some(actions) = doc.get_mut("actions").and_then(Value::as_array_mut) {
        actions.retain(|action| {
            action
                .get("module")
                .and_then(Value::as_str)
                .is_some_and(|module| allowed_packages.contains(module))
        });
    }
    let mut stripped_any = false;
    if let Some(advisories) = doc.get_mut("advisories").and_then(Value::as_object_mut) {
        let before = advisories.len();
        advisories.retain(|_, advisory| {
            advisory
                .get("module_name")
                .and_then(Value::as_str)
                .is_some_and(|module| allowed_packages.contains(module))
        });
        stripped_any = advisories.len() != before;
    }
    if stripped_any {
        recompute_vulnerability_counts(doc);
    }
}

pub(super) fn merge_upstream_legacy_responses(
    responses: Vec<(HashSet<String>, Map<String, Value>)>,
) -> Map<String, Value> {
    let mut merged_actions = Vec::new();
    let mut merged_advisories = Map::new();
    let mut merged_muted = Vec::new();
    let mut counts = LegacyAuditMetadata::default();
    let mut next_suffix = HashMap::new();

    for (allowed, mut doc) in responses {
        sanitize_upstream_legacy_response(&mut doc, &allowed);
        if doc.contains_key("advisories") && !has_vulnerability_counts(&doc) {
            recompute_vulnerability_counts(&mut doc);
        }
        append_json_array(&mut merged_actions, doc.remove("actions"));
        append_json_array(&mut merged_muted, doc.remove("muted"));
        if let Some(Value::Object(advisories)) = doc.remove("advisories") {
            insert_merged_advisories(&mut merged_advisories, advisories, &mut next_suffix);
        }
        accumulate_upstream_metadata(&mut counts, doc.get("metadata"));
    }

    let mut result = Map::new();
    result.insert("actions".to_string(), Value::Array(merged_actions));
    result.insert("advisories".to_string(), Value::Object(merged_advisories));
    result.insert("muted".to_string(), Value::Array(merged_muted));
    result.insert(
        "metadata".to_string(),
        serde_json::to_value(&counts).unwrap_or_else(|_| json!({})),
    );
    result
}

fn has_vulnerability_counts(doc: &Map<String, Value>) -> bool {
    doc.get("metadata")
        .and_then(|meta| meta.get("vulnerabilities"))
        .and_then(Value::as_object)
        .is_some()
}

fn append_json_array(target: &mut Vec<Value>, maybe_array: Option<Value>) {
    if let Some(Value::Array(items)) = maybe_array {
        target.extend(items);
    }
}

fn insert_merged_advisories(
    target: &mut Map<String, Value>,
    incoming: Map<String, Value>,
    next_suffix: &mut HashMap<String, usize>,
) {
    for (key, advisory) in incoming {
        if !target.contains_key(&key) {
            target.insert(key, advisory);
            continue;
        }
        let suffix_cursor = next_suffix.entry(key.clone()).or_insert(2);
        loop {
            let suffix = *suffix_cursor;
            *suffix_cursor += 1;
            let candidate = format!("{key}-{suffix}");
            if !target.contains_key(&candidate) {
                target.insert(candidate, advisory);
                break;
            }
        }
    }
}

fn accumulate_upstream_metadata(counts: &mut LegacyAuditMetadata, raw_metadata: Option<&Value>) {
    let Some(meta) = raw_metadata.and_then(Value::as_object) else {
        return;
    };
    counts.dependencies += json_usize(meta, "dependencies");
    counts.dev_dependencies += json_usize(meta, "devDependencies");
    counts.optional_dependencies += json_usize(meta, "optionalDependencies");
    counts.total_dependencies += json_usize(meta, "totalDependencies");
    let Some(vulns) = meta.get("vulnerabilities").and_then(Value::as_object) else {
        return;
    };
    counts.vulnerabilities.info += json_usize(vulns, "info");
    counts.vulnerabilities.low += json_usize(vulns, "low");
    counts.vulnerabilities.moderate += json_usize(vulns, "moderate");
    counts.vulnerabilities.high += json_usize(vulns, "high");
    counts.vulnerabilities.critical += json_usize(vulns, "critical");
    counts.vulnerabilities.total += json_usize(vulns, "total");
}

fn recompute_vulnerability_counts(doc: &mut Map<String, Value>) {
    let mut counts = LegacyVulnerabilityCounts::default();
    if let Some(advisories) = doc.get("advisories").and_then(Value::as_object) {
        for advisory in advisories.values() {
            let severity = advisory
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("moderate");
            counts.record_severity(severity);
        }
    }
    let metadata = doc
        .entry("metadata".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(meta_obj) = metadata
        && let Ok(vulns_val) = serde_json::to_value(&counts)
    {
        meta_obj.insert("vulnerabilities".to_string(), vulns_val);
    }
}

fn json_usize(obj: &Map<String, Value>, key: &str) -> usize {
    obj.get(key)
        .and_then(Value::as_u64)
        .and_then(|num| usize::try_from(num).ok())
        .unwrap_or(0)
}
