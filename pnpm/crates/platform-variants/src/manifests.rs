//! The manifests of a resolved version's variants.

use super::{Platform, variant_tag};
use serde_json::Value;

/// The manifest of each selectable variant a resolved version's manifest
/// lists under `dist.pnpr.variants`, by the tag its platforms give: the
/// version's manifest with the variant's differing fields applied, each
/// replacing the version's field, `null` removing it. A variant with a
/// platform this vocabulary does not know is left out.
#[must_use]
pub fn variant_manifests(manifest: &Value) -> Vec<(String, Value)> {
    let Some(variants) = manifest.pointer("/dist/pnpr/variants").and_then(Value::as_array) else {
        return Vec::new();
    };
    variants
        .iter()
        .filter_map(|variant| {
            let platforms: Vec<Platform> =
                serde_json::from_value(variant.get("platforms")?.clone()).ok()?;
            let tag = variant_tag(&platforms).ok()?;
            let mut effective = manifest.clone();
            let fields = effective.as_object_mut()?;
            let difference = variant.get("manifest").and_then(Value::as_object);
            for (field, value) in difference.into_iter().flatten() {
                if value.is_null() {
                    fields.remove(field);
                } else {
                    fields.insert(field.clone(), value.clone());
                }
            }
            Some((tag, effective))
        })
        .collect()
}
