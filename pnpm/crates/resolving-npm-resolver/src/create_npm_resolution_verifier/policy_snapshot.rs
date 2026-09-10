use super::{
    BTreeMap, CreateNpmResolutionVerifierOptions, DateTime, Digest, HashMap, JsonValue, Sha256,
    TrustPolicy, Utc,
};

pub(super) fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut deduped: Vec<String> = values.to_vec();
    deduped.sort();
    deduped.dedup();
    deduped
}

pub(super) fn named_registries_routing_digest(
    registries_by_prefix: &HashMap<String, String>,
) -> String {
    let sorted: BTreeMap<&str, &str> = registries_by_prefix
        .iter()
        .map(|(alias, registry)| (alias.as_str(), registry.as_str()))
        .collect();
    let encoded = serde_json::to_vec(&sorted).expect("named registry mappings are serializable");
    format!("{:x}", Sha256::digest(encoded))
}

/// Argument bundle for [`build_policy_snapshot`].
#[derive(Clone, Copy)]
pub(super) struct BuildPolicySnapshot<'a> {
    pub(super) minimum_release_age: u64,
    pub(super) sorted_min_age_excludes: &'a [String],
    pub(super) ignore_missing_time_field: bool,
    pub(super) trust_policy: Option<TrustPolicy>,
    pub(super) sorted_trust_excludes: &'a [String],
    pub(super) trust_policy_ignore_after: Option<u64>,
    pub(super) named_registries_routing: &'a str,
}

pub(super) fn build_policy_snapshot(
    opts: &BuildPolicySnapshot<'_>,
) -> serde_json::Map<String, JsonValue> {
    let mut map = serde_json::Map::new();
    // Marks runs that enforced the (unconditional) tarball-URL binding so
    // `can_trust_past_check` rejects pre-rule cache records and re-verifies.
    map.insert("tarballUrlBinding".to_string(), JsonValue::Bool(true));
    map.insert("revisionHistoryBinding".to_string(), JsonValue::Bool(true));
    // Same cache identity rule for the missing-integrity structural check.
    map.insert("integrityRequired".to_string(), JsonValue::Bool(true));
    map.insert(
        "namedRegistriesRouting".to_string(),
        JsonValue::String(opts.named_registries_routing.to_string()),
    );
    map.insert("minimumReleaseAge".to_string(), JsonValue::from(opts.minimum_release_age));
    map.insert(
        "minimumReleaseAgeExclude".to_string(),
        policy_patterns_json(opts.sorted_min_age_excludes),
    );
    map.insert(
        "trustPolicy".to_string(),
        match opts.trust_policy {
            Some(TrustPolicy::NoDowngrade) => JsonValue::String("no-downgrade".to_string()),
            Some(TrustPolicy::Off) | None => JsonValue::Null,
        },
    );
    map.insert("trustPolicyExclude".to_string(), policy_patterns_json(opts.sorted_trust_excludes));
    map.insert(
        "trustPolicyIgnoreAfter".to_string(),
        match opts.trust_policy_ignore_after {
            Some(value) => JsonValue::from(value),
            None => JsonValue::Null,
        },
    );
    map.insert(
        "minimumReleaseAgeIgnoreMissingTime".to_string(),
        JsonValue::Bool(opts.ignore_missing_time_field),
    );
    map
}

/// Checked arithmetic makes an unrepresentable age disable the cutoff rather
/// than wrap into a date in the wrong direction.
pub(super) fn minimum_release_age_cutoff(
    opts: &CreateNpmResolutionVerifierOptions,
) -> Option<DateTime<Utc>> {
    let age_check_active = opts.minimum_release_age.is_some_and(|minutes| minutes > 0);

    if age_check_active {
        let minutes = opts.minimum_release_age.unwrap_or(0);
        let now = opts.now.unwrap_or_else(Utc::now);
        i64::try_from(minutes)
            .ok()
            .and_then(chrono::Duration::try_minutes)
            .and_then(|duration| now.checked_sub_signed(duration))
    } else {
        None
    }
}

pub(super) fn cached_policy_patterns(
    policy: &serde_json::Map<String, JsonValue>,
    key: &str,
) -> Vec<String> {
    policy
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values.iter().filter_map(|value| value.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default()
}

pub(super) fn policy_patterns_json(patterns: &[String]) -> JsonValue {
    JsonValue::Array(patterns.iter().map(|spec| JsonValue::String(spec.clone())).collect())
}
