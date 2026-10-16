use chrono::{DateTime, Utc};
use serde_json::Value;

/// Top-level packument fields *copied verbatim* into the abbreviated
/// (`application/vnd.npm.install-v1+json`) form.
pub(super) const ABBREVIATED_TOP_FIELDS: &[&str] = &["name", "dist-tags"];

/// Per-version fields preserved in the abbreviated form — a subset of
/// the npm spec's abbreviated version object.
pub(super) const ABBREVIATED_VERSION_FIELDS: &[&str] = &[
    "name",
    "version",
    "deprecated",
    "bin",
    "dist",
    "engines",
    "directories",
    "dependencies",
    "peerDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "cpu",
    "os",
    "libc",
    "peerDependenciesMeta",
    "hasInstallScript",
];

/// Strip a parsed packument down to the abbreviated install-v1 form.
/// Should be called *after* [`crate::rewrite_tarball_urls`] so the returned
/// document's `dist.tarball` URLs already point at this server.
pub fn abbreviate_packument(packument: &Value, now: DateTime<Utc>) -> Value {
    let mut out = serde_json::Map::new();
    let Some(obj) = packument.as_object() else {
        return Value::Object(out);
    };
    copy_fields(&mut out, obj, ABBREVIATED_TOP_FIELDS);
    if let Some(time) = obj.get("time").and_then(Value::as_object) {
        let time = crate::time::coarsen_time_map(time, now);
        if let Some(modified) = time.get("modified") {
            out.insert("modified".to_string(), modified.clone());
        }
        out.insert("time".to_string(), Value::Object(time));
    }
    if let Some(versions) = obj.get("versions").and_then(Value::as_object) {
        out.insert("versions".to_string(), abbreviate_versions(versions));
    }
    Value::Object(out)
}

/// The `versions` map with each manifest cut down to the fields a resolver reads.
pub(super) fn abbreviate_versions(versions: &serde_json::Map<String, Value>) -> Value {
    let mut abbreviated = serde_json::Map::with_capacity(versions.len());
    for (version_id, version_value) in versions {
        let Some(version_obj) = version_value.as_object() else { continue };
        let mut trimmed = serde_json::Map::new();
        copy_fields(&mut trimmed, version_obj, ABBREVIATED_VERSION_FIELDS);
        trim_dist_fields(&mut trimmed);
        abbreviated.insert(version_id.clone(), Value::Object(trimmed));
    }
    Value::Object(abbreviated)
}

/// Copy whichever of `fields` the source carries.
pub(super) fn copy_fields(
    out: &mut serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
    fields: &[&str],
) {
    for &field in fields {
        if let Some(value) = source.get(field) {
            out.insert(field.to_string(), value.clone());
        }
    }
}

/// Trim `dist` subfields the resolver and installer never read.
pub(super) fn trim_dist_fields(version: &mut serde_json::Map<String, Value>) {
    let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    dist.remove("npm-signature");
    if dist
        .get("integrity")
        .and_then(Value::as_str)
        .is_some_and(|integrity| !integrity.is_empty())
    {
        dist.remove("shasum");
    }
}
