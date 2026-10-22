use pnpr_upstream::tarball_basename;
use serde_json::Map;

use super::{CanonicalPackageName, RegistryError, Value};

#[derive(Default)]
pub(super) struct PendingRestores {
    fields: Vec<(String, String, Value)>,
    dist: Vec<(String, String, Value)>,
}

impl PendingRestores {
    fn apply(self, incoming: &mut Value) {
        for (version, key, value) in self.fields {
            if let Some(manifest) = incoming
                .get_mut("versions")
                .and_then(|versions| versions.get_mut(&version))
                .and_then(Value::as_object_mut)
            {
                manifest.insert(key, value);
            }
        }
        for (version, key, value) in self.dist {
            if let Some(dist) = incoming
                .get_mut("versions")
                .and_then(|versions| versions.get_mut(&version))
                .and_then(|manifest| manifest.get_mut("dist"))
                .and_then(Value::as_object_mut)
            {
                dist.insert(key, value);
            }
        }
    }
}

fn validate_published_version(
    entry: &PublishedVersion<'_>,
    name: &CanonicalPackageName,
    restores: &mut PendingRestores,
) -> Option<RegistryError> {
    if let Some(err) = check_version_fields_immutable(entry, restores) {
        return Some(err);
    }
    if let Some(err) = check_integrity_immutable(entry, &mut restores.dist) {
        return Some(err);
    }
    check_tarball_immutable(entry, name, &mut restores.dist)
}

/// Hold a published version's security-critical `dist` fields immutable across
/// the partial-unpublish `PUT`, which otherwise persists the body verbatim.
/// [`super::super::expected_tarball_dist`] resolves a tarball request to a version by
/// `dist.tarball` basename and verifies the bytes against that version's string
/// `dist.integrity`, so letting either drift — while the bytes on disk stay put —
/// breaks installs of that version (`EINTEGRITY`, or a 404/502 redirect).
///
/// For each version in the body, given a hosted packument: changing the
/// `dist.integrity` or `dist.tarball` basename of an already-published version is
/// rejected; omitting either is repaired from the hosted value (the round-trip
/// drops them on retained versions); and a version not already published is
/// rejected — this endpoint only removes versions, and an added entry could
/// collide a basename or seed a tarball-less one. A `PUT` to a package with no
/// hosted packument is rejected outright (nothing to unpublish, and the write
/// would seed versions that publish can never overwrite).
///
/// Returns the rejection, or `None` when the body is acceptable (after any
/// restores). Must hold the package lock so a concurrent publish can't race it.
pub(super) fn enforce_published_version_immutability(
    hosted: &Value,
    name: &CanonicalPackageName,
    incoming: &mut Value,
) -> Option<RegistryError> {
    let Some(incoming_versions) = incoming.get_mut("versions").and_then(Value::as_object_mut)
    else {
        return Some(RegistryError::BadRequest {
            reason: "packument must contain an object-valued versions field".to_string(),
        });
    };
    // `GET /:pkg` overlays `dist.pnpr` from the server's tar-integrity table
    // while the stored packument omits it (see `pnpr_storage::publish`). Strip
    // any round-tripped or client-supplied `dist.pnpr` before comparing or
    // persisting.
    for manifest in incoming_versions.values_mut() {
        if let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) {
            dist.remove("pnpr");
        }
    }
    let hosted_versions = hosted.get("versions").and_then(Value::as_object);
    let mut restores = PendingRestores::default();
    for (version, manifest) in &*incoming_versions {
        let Some(existing) = hosted_versions.and_then(|versions| versions.get(version)) else {
            return Some(RegistryError::BadRequest {
                reason: format!(
                    "version {version:?} is not in the published package; this endpoint removes versions, it does not add them",
                ),
            });
        };
        let entry = PublishedVersion { version, existing, manifest };
        if let Some(err) = validate_published_version(&entry, name, &mut restores) {
            return Some(err);
        }
    }
    restores.apply(incoming);
    None
}

/// The packument a rewrite submitted, with the fields only the store owns
/// stripped.
///
/// The write destination is the URL package name; a mismatched body name would
/// otherwise land under the URL package and persist an inconsistent manifest.
pub(super) fn submitted_packument(
    body: &[u8],
    name: &CanonicalPackageName,
) -> Result<Value, RegistryError> {
    let mut packument: Value = serde_json::from_slice(body).map_err(RegistryError::Json)?;
    if let Some(body_name) = packument.get("name").and_then(Value::as_str)
        && body_name != name.as_str()
    {
        return Err(RegistryError::BadRequest {
            reason: format!(
                "packument name {body_name:?} does not match the URL package {:?}",
                name.as_str(),
            ),
        });
    }
    if let Some(obj) = packument.as_object_mut() {
        obj.remove("_attachments");
        obj.remove("_rev");
        obj.remove("_revisions");
    }
    Ok(packument)
}

/// One version of an update, beside the version the store already holds.
pub(super) struct PublishedVersion<'a> {
    pub(super) version: &'a String,
    pub(super) existing: &'a Value,
    pub(super) manifest: &'a Value,
}

/// `dist.integrity` of a published version may not change, and an update that
/// omits it has the stored one put back.
pub(super) fn check_integrity_immutable(
    entry: &PublishedVersion<'_>,
    restore: &mut Vec<(String, String, Value)>,
) -> Option<RegistryError> {
    let version = entry.version;
    // A present dist.integrity must be a string; a non-string would slip past
    // the string-only checks below.
    let incoming = match entry.manifest
        .get("dist")
        .and_then(|dist| dist.get("integrity"))
    {
        None => None,
        Some(Value::String(value)) => Some(value.as_str()),
        Some(_) => {
            return Some(RegistryError::BadRequest {
                reason: format!("dist.integrity for version {version:?} must be a string"),
            });
        }
    };
    let stored = entry.existing
        .get("dist")
        .and_then(|dist| dist.get("integrity"))
        .and_then(Value::as_str)?;
    match incoming {
        Some(submitted) if submitted != stored => Some(RegistryError::BadRequest {
            reason: format!("dist.integrity for the published version {version:?} is immutable"),
        }),
        Some(_) => None,
        None => {
            let refusal = require_object_dist(entry.manifest, version);
            if refusal.is_none() {
                restore.push((
                    version.clone(),
                    "integrity".to_string(),
                    Value::String(stored.to_string()),
                ));
            }
            refusal
        }
    }
}

/// `dist.tarball` of a published version may not change, and an update that
/// omits it has the stored one put back.
///
/// Basenames are compared, not URLs: the round-trip carries the rewritten URL
/// (see [`pnpr_upstream::rewrite_tarball_urls`]) while the hosted side keeps
/// the original, and [`served_tarball_basename`] applies the same
/// version-derived fallback so a basename-less stored URL is still pinned.
pub(super) fn check_tarball_immutable(
    entry: &PublishedVersion<'_>,
    name: &CanonicalPackageName,
    restore: &mut Vec<(String, String, Value)>,
) -> Option<RegistryError> {
    let version = entry.version;
    let stored_basename = served_tarball_basename(entry.existing, name)?;
    let incoming_basename = entry.manifest
        .get("dist")
        .and_then(|dist| dist.get("tarball"))
        .and_then(Value::as_str)
        .and_then(tarball_basename);
    match incoming_basename {
        Some(submitted) if submitted != stored_basename => Some(RegistryError::BadRequest {
            reason: format!("dist.tarball for the published version {version:?} is immutable"),
        }),
        Some(_) => None,
        None => {
            let refusal = require_object_dist(entry.manifest, version);
            if refusal.is_none() {
                let stored = entry.existing
                    .get("dist")
                    .and_then(|dist| dist.get("tarball"))
                    .cloned()
                    .unwrap_or(Value::Null);
                restore.push((version.clone(), "tarball".to_string(), stored));
            }
            refusal
        }
    }
}

/// The tarball basename a version is actually served under, mirroring
/// [`pnpr_upstream::rewrite_tarball_urls`]: the `dist.tarball` URL's own basename when it has
/// one, otherwise the version-derived canonical name the rewrite falls back to.
/// Returns `None` when the manifest carries no string `dist.tarball` to serve.
pub(super) fn served_tarball_basename(
    manifest: &Value,
    pkg: &CanonicalPackageName,
) -> Option<String> {
    let url = manifest
        .get("dist")
        .and_then(|dist| dist.get("tarball"))
        .and_then(Value::as_str)?;
    if let Some(basename) = tarball_basename(url) {
        return Some(basename.to_owned());
    }
    let version = manifest.get("version").and_then(Value::as_str)?;
    Some(pkg.tarball_name_for_version(version))
}

/// Reject a published version whose `dist` isn't an object: a restore needs an
/// object to write into, so otherwise it would no-op and persist the version
/// without the field — the stripping this guards against.
pub(super) fn require_object_dist(manifest: &Value, version: &str) -> Option<RegistryError> {
    if manifest.get("dist").is_some_and(Value::is_object) {
        return None;
    }
    Some(RegistryError::BadRequest {
        reason: format!("dist for the published version {version:?} must be an object"),
    })
}

/// All fields of a published version manifest — other than `deprecated` and
/// `dist` (which has dedicated integrity and tarball checks) — are immutable.
/// Modifications to `dependencies`, `scripts`, `bin`, or any other metadata
fn check_existing_fields(
    version: &str,
    existing: &Map<String, Value>,
    manifest: &Map<String, Value>,
    restore_fields: &mut Vec<(String, String, Value)>,
) -> Option<RegistryError> {
    for (key, existing_val) in existing {
        if key == "deprecated" || key == "dist" {
            continue;
        }
        match manifest.get(key) {
            Some(incoming_val) if incoming_val != existing_val => {
                return Some(RegistryError::BadRequest {
                    reason: format!(
                        "field {key:?} for the published version {version:?} is immutable",
                    ),
                });
            }
            Some(_) => {}
            None => {
                restore_fields.push((version.to_string(), key.clone(), existing_val.clone()));
            }
        }
    }
    None
}

fn check_no_extra_fields(
    version: &str,
    existing: &Map<String, Value>,
    manifest: &Map<String, Value>,
) -> Option<RegistryError> {
    for key in manifest.keys() {
        if key == "deprecated" || key == "dist" {
            continue;
        }
        if !existing.contains_key(key) {
            return Some(RegistryError::BadRequest {
                reason: format!("field {key:?} for the published version {version:?} is immutable"),
            });
        }
    }
    None
}

fn check_dist_fields_immutable(
    version: &str,
    existing: &Value,
    manifest: &Value,
    restore_dist: &mut Vec<(String, String, Value)>,
) -> Option<RegistryError> {
    let (Some(existing_dist), Some(manifest_dist)) = (
        existing.get("dist").and_then(Value::as_object),
        manifest.get("dist").and_then(Value::as_object),
    ) else {
        return None;
    };
    if let Some(err) =
        check_existing_dist_fields(version, existing_dist, manifest_dist, restore_dist)
    {
        return Some(err);
    }
    check_no_extra_dist_fields(version, existing_dist, manifest_dist)
}

fn check_existing_dist_fields(
    version: &str,
    existing_dist: &Map<String, Value>,
    manifest_dist: &Map<String, Value>,
    restore_dist: &mut Vec<(String, String, Value)>,
) -> Option<RegistryError> {
    for (key, stored) in existing_dist {
        if matches!(key.as_str(), "integrity" | "tarball") {
            continue;
        }
        match manifest_dist.get(key) {
            Some(incoming) if incoming != stored => {
                return Some(RegistryError::BadRequest {
                    reason: format!(
                        "dist.{key} for the published version {version:?} is immutable",
                    ),
                });
            }
            Some(_) => {}
            None => restore_dist.push((version.to_string(), key.clone(), stored.clone())),
        }
    }
    None
}

fn check_no_extra_dist_fields(
    version: &str,
    existing_dist: &Map<String, Value>,
    manifest_dist: &Map<String, Value>,
) -> Option<RegistryError> {
    for key in manifest_dist.keys() {
        if !existing_dist.contains_key(key) && !matches!(key.as_str(), "integrity" | "tarball") {
            return Some(RegistryError::BadRequest {
                reason: format!("dist.{key} for the published version {version:?} is immutable"),
            });
        }
    }
    None
}

/// All fields of a published version manifest — other than `deprecated` and
/// `dist` (which has dedicated integrity and tarball checks) — are immutable.
/// Modifications to `dependencies`, `scripts`, `bin`, or any other metadata
/// must be rejected.
pub(super) fn check_version_fields_immutable(
    entry: &PublishedVersion<'_>,
    restores: &mut PendingRestores,
) -> Option<RegistryError> {
    let version = entry.version;
    let existing_obj = entry.existing.as_object()?;
    let Some(manifest_obj) = entry.manifest.as_object() else {
        return Some(RegistryError::BadRequest {
            reason: format!("manifest for the published version {version:?} must be an object"),
        });
    };

    if let Some(err) =
        check_existing_fields(version, existing_obj, manifest_obj, &mut restores.fields)
    {
        return Some(err);
    }
    if let Some(err) = check_no_extra_fields(version, existing_obj, manifest_obj) {
        return Some(err);
    }
    check_dist_fields_immutable(version, entry.existing, entry.manifest, &mut restores.dist)
}

#[cfg(test)]
mod tests;
