use super::{AppState, Arc, CanonicalPackageName, HashSet, RegistryError, Value};

pub(super) fn filter_osv_vulnerable_versions(
    packument: &mut Value,
    name: &CanonicalPackageName,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let package_name = name.as_str();
    let mut blocked_keys = HashSet::new();
    let mut retained_version_keys = HashSet::new();
    let has_time = packument
        .get("time")
        .and_then(Value::as_object)
        .is_some();
    if let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) {
        versions.retain(|key, manifest| {
            if version_is_vulnerable(osv_index, package_name, key, manifest) {
                blocked_keys.insert(key.clone());
                return false;
            }
            if has_time {
                retained_version_keys.insert(key.clone());
            }
            true
        });
    }
    drop_blocked_dist_tags(packument, osv_index, package_name, &blocked_keys);
    drop_blocked_time_entries(
        packument,
        osv_index,
        package_name,
        &BlockedVersions { blocked: &blocked_keys, retained: &retained_version_keys },
    );
}

/// Drop every tag pointing at a version an advisory covers.
pub(super) fn drop_blocked_dist_tags(
    packument: &mut Value,
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    blocked: &HashSet<String>,
) {
    let Some(tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) else {
        return;
    };
    tags.retain(|_, version| {
        version
            .as_str()
            .is_none_or(|version| {
                !blocked.contains(version) && !osv_index.is_vulnerable(package_name, version)
            })
    });
}

/// The versions a filter pass blocked, and the ones it kept.
pub(super) struct BlockedVersions<'a> {
    pub(super) blocked: &'a HashSet<String>,
    pub(super) retained: &'a HashSet<String>,
}

/// Drop the `time` entries of blocked versions, keeping the two document-level
/// stamps and every version the filter kept.
pub(super) fn drop_blocked_time_entries(
    packument: &mut Value,
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    versions: &BlockedVersions<'_>,
) {
    let Some(time) = packument.get_mut("time").and_then(Value::as_object_mut) else {
        return;
    };
    time.retain(|key, _| {
        !versions.blocked.contains(key)
            && (matches!(key.as_str(), "created" | "modified")
                || versions.retained.contains(key)
                || !osv_index.is_vulnerable(package_name, key))
    });
}

/// Whether an advisory covers a packument entry, under either the version its
/// key names or the one its manifest declares.
pub(super) fn version_is_vulnerable(
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    key: &str,
    manifest: &Value,
) -> bool {
    if osv_index.is_vulnerable(package_name, key) {
        return true;
    }
    manifest
        .get("version")
        .and_then(Value::as_str)
        .is_some_and(|version| version != key && osv_index.is_vulnerable(package_name, version))
}

pub(super) fn filter_osv_vulnerable_dist_tags(
    tags: &mut Value,
    packument: &Value,
    name: &CanonicalPackageName,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let Some(tags) = tags.as_object_mut() else {
        return;
    };
    let package_name = name.as_str();
    tags.retain(|_, version| {
        version
            .as_str()
            .is_none_or(|version| {
                !is_osv_vulnerable_packument_version(packument, package_name, version, osv_index)
            })
    });
}

pub(super) fn is_osv_vulnerable_packument_version(
    packument: &Value,
    package_name: &str,
    version: &str,
    osv_index: &pnpr_osv::OsvIndex,
) -> bool {
    if osv_index.is_vulnerable(package_name, version) {
        return true;
    }
    let manifest_version = packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .and_then(|manifest| manifest.get("version"))
        .and_then(Value::as_str);
    manifest_version.is_some_and(|manifest_version| {
        manifest_version != version && osv_index.is_vulnerable(package_name, manifest_version)
    })
}

pub(super) fn resolve_version_or_tag<'a>(packument: &'a Value, version_or_tag: &'a str) -> &'a str {
    packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag)
}

pub(super) fn ensure_osv_allowed(
    state: &AppState,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<(), RegistryError> {
    let Some(osv_index) = state.inner.osv_index.as_ref() else {
        return Ok(());
    };
    let ids = osv_index.vulnerability_ids(name.as_str(), version);
    if ids.is_empty() {
        return Ok(());
    }
    Err(RegistryError::OsvVulnerability {
        package: name.as_str().to_string(),
        version: version.to_string(),
        advisories: pnpr_osv::format_advisory_ids(&ids),
    })
}
