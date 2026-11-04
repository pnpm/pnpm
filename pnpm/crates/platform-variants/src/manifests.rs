//! The manifests of a resolved version's variants.

use super::{Platform, variant_tag};
use serde_json::{Map, Value};

/// The manifest of each selectable variant a resolved version's manifest
/// lists under `dist.pnpr.variants`, by the tag its platforms give: the
/// version's manifest with the variant's differing fields applied, each
/// replacing the version's field, `null` removing it. A variant with a
/// platform this vocabulary does not know is left out.
///
/// A variant's manifest has no `dist`: it describes the variant's own
/// archive, not the version's, and copying the version's `dist` would copy
/// the whole variant list into each variant.
#[must_use]
pub fn variant_manifests(manifest: &Value) -> Vec<(String, Value)> {
    let Some(variants) = manifest.pointer("/dist/pnpr/variants").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(fields) = manifest.as_object() else { return Vec::new() };
    let base: Map<String, Value> = fields
        .iter()
        .filter(|(field, _)| field.as_str() != "dist")
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect();
    variants
        .iter()
        .filter_map(|variant| variant_manifest(&base, variant))
        .collect()
}

/// One variant's tag and manifest, from the version's manifest without its
/// `dist`.
fn variant_manifest(base: &Map<String, Value>, variant: &Value) -> Option<(String, Value)> {
    let platforms: Vec<Platform> = serde_json::from_value(variant.get("platforms")?.clone()).ok()?;
    let tag = variant_tag(&platforms).ok()?;
    let mut fields = base.clone();
    let difference = variant.get("manifest").and_then(Value::as_object);
    for (field, value) in difference.into_iter().flatten() {
        if value.is_null() {
            fields.remove(field);
        } else {
            fields.insert(field.clone(), value.clone());
        }
    }
    Some((tag, Value::Object(fields)))
}
