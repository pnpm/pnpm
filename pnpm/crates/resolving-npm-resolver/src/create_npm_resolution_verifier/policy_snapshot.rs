use super::{
    BTreeMap, DateTime, Digest, HashMap, JsonValue, ReleaseAgeCheck, Sha256, TrustCheck,
    TrustPolicy, Utc, VerificationMetadataClient, VerificationRegistryRoutes,
    VerificationReleaseAgeOptions, normalized_registry_key,
};

pub(super) fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut deduped: Vec<String> = values.to_vec();
    deduped.sort();
    deduped.dedup();
    deduped
}

fn named_registries_routing_digest(registries_by_prefix: &HashMap<String, String>) -> String {
    let sorted: BTreeMap<&str, &str> = registries_by_prefix
        .iter()
        .map(|(alias, registry)| (alias.as_str(), registry.as_str()))
        .collect();
    let encoded = serde_json::to_vec(&sorted).expect("named registry mappings are serializable");
    format!("{:x}", Sha256::digest(encoded))
}

fn registries_routing_digest(registries: &HashMap<String, String>) -> String {
    let sorted: BTreeMap<&str, String> = registries
        .iter()
        .map(|(scope, registry)| (scope.as_str(), canonical_registry_url(registry)))
        .collect();
    let encoded = serde_json::to_vec(&sorted).expect("registry mappings are serializable");
    format!("{:x}", Sha256::digest(encoded))
}

fn canonical_registry_url(registry: &str) -> String {
    normalized_registry_key(registry)
        .filter(|key| !key.starts_with("null/"))
        .unwrap_or_else(|| {
            if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
        })
}

fn registry_options_digest(
    registry_options_by_url: Option<&BTreeMap<String, pnpm_lockfile::RegistryOptions>>,
    registry_supports_time_field: bool,
) -> String {
    let non_default: BTreeMap<&str, &pnpm_lockfile::RegistryOptions> = registry_options_by_url
        .into_iter()
        .flatten()
        .filter(|(_, options)| **options != pnpm_lockfile::RegistryOptions::default())
        .map(|(url, options)| (url.as_str(), options))
        .collect();
    let encoded = serde_json::to_vec(&(registry_supports_time_field, non_default))
        .expect("registry options are serializable");
    format!("{:x}", Sha256::digest(encoded))
}

fn insert_structural_and_routing_policy(
    map: &mut serde_json::Map<String, JsonValue>,
    routing: &VerificationRegistryRoutes,
    registry_supports_time_field: bool,
) {
    // Marks runs that enforced the (unconditional) tarball-URL binding so
    // `can_trust_past_check` rejects pre-rule cache records and re-verifies.
    map.insert("tarballUrlBinding".to_string(), JsonValue::Bool(true));
    map.insert("revisionHistoryBinding".to_string(), JsonValue::Bool(true));
    // Same cache identity rule for the missing-integrity structural check.
    map.insert("integrityRequired".to_string(), JsonValue::Bool(true));
    map.insert("variationResolutionsVerified".to_string(), JsonValue::Bool(true));
    map.insert(
        "namedRegistriesRouting".to_string(),
        JsonValue::String(named_registries_routing_digest(&routing.registries_by_prefix)),
    );
    map.insert(
        "registriesRouting".to_string(),
        JsonValue::String(registries_routing_digest(&routing.registries)),
    );
    map.insert(
        "registryOptions".to_string(),
        JsonValue::String(registry_options_digest(
            routing.registry_options_by_url.as_ref(),
            registry_supports_time_field,
        )),
    );
}

pub(super) fn build_policy_snapshot(
    release_age: &ReleaseAgeCheck,
    trust: &TrustCheck,
    routing: &VerificationRegistryRoutes,
    metadata: &VerificationMetadataClient,
) -> serde_json::Map<String, JsonValue> {
    let mut map = serde_json::Map::new();
    insert_structural_and_routing_policy(&mut map, routing, metadata.registry_supports_time_field);
    map.insert(
        "minimumReleaseAge".to_string(),
        JsonValue::from(release_age.minimum_minutes.unwrap_or(0)),
    );
    map.insert(
        "minimumReleaseAgeExclude".to_string(),
        policy_patterns_json(&release_age.sorted_excludes),
    );
    map.insert(
        "trustPolicy".to_string(),
        match trust.policy {
            Some(TrustPolicy::NoDowngrade) => JsonValue::String("no-downgrade".to_string()),
            Some(TrustPolicy::Off) | None => JsonValue::Null,
        },
    );
    map.insert("trustPolicyExclude".to_string(), policy_patterns_json(&trust.sorted_excludes));
    map.insert(
        "trustPolicyIgnoreAfter".to_string(),
        match trust.ignore_after {
            Some(value) => JsonValue::from(value),
            None => JsonValue::Null,
        },
    );
    map.insert(
        "minimumReleaseAgeIgnoreMissingTime".to_string(),
        JsonValue::Bool(metadata.ignore_missing_time_field),
    );
    map
}

/// Checked arithmetic makes an unrepresentable age disable the cutoff rather
/// than wrap into a date in the wrong direction.
pub(super) fn minimum_release_age_cutoff(
    opts: &VerificationReleaseAgeOptions,
    now: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let age_check_active = opts.minimum_minutes.is_some_and(|minutes| minutes > 0);

    if age_check_active {
        let minutes = opts.minimum_minutes.unwrap_or(0);
        let now = now.unwrap_or_else(Utc::now);
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
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn policy_patterns_json(patterns: &[String]) -> JsonValue {
    JsonValue::Array(
        patterns
            .iter()
            .map(|spec| JsonValue::String(spec.clone()))
            .collect(),
    )
}
