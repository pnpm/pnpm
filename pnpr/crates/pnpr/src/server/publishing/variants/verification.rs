//! Checking published variants against their primary.

use super::{
    Declarations, Map, PublishedVariant, RegistryError, STORED_FIELD, Value, VariantDeclaration,
    bad_request, declarations::PLATFORM_FIELDS,
};
use std::collections::HashSet;

/// Dependency fields every variant must share with the primary.
const DEPENDENCY_FIELDS: &[&str] =
    &["dependencies", "optionalDependencies", "peerDependencies", "peerDependenciesMeta"];

/// Check every published variant against its primary and record the verified
/// variants on the merged document.
pub(super) fn attach_variants(
    merged: &mut Value,
    declared: &Declarations,
    published: &[PublishedVariant],
) -> Result<(), RegistryError> {
    for (version, variants) in declared {
        let pointer = format!("/versions/{}", escape_pointer(version));
        let primary = merged
            .pointer(&pointer)
            .cloned()
            .ok_or_else(|| {
                bad_request(format!("variants declared for unknown version {version}"))
            })?;
        let stored = variants
            .iter()
            .map(|variant| stored_variant(version, variant, &primary, published))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(manifest) = merged.pointer_mut(&pointer).and_then(Value::as_object_mut) {
            manifest.insert(STORED_FIELD.to_string(), Value::Array(stored));
        }
    }
    Ok(())
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// What the stored version keeps of one verified variant.
fn stored_variant(
    version: &str,
    variant: &VariantDeclaration,
    primary: &Value,
    published: &[PublishedVariant],
) -> Result<Value, RegistryError> {
    let Some(found) = published
        .iter()
        .find(|item| item.version == *version && item.tar.tag == variant.tag)
    else {
        return Err(bad_request(format!(
            "variant {:?} of {version} is declared but not attached",
            variant.tag,
        )));
    };
    check_invariants(version, variant, primary, found)?;
    let mut entry = Map::new();
    entry.insert("tag".into(), Value::String(variant.tag.clone()));
    entry.insert("platforms".into(), Value::Array(variant.platforms.clone()));
    entry.insert("integrity".into(), Value::String(found.tar.tar_integrity.clone()));
    if let Some(size) = found.inspection.unpacked_size {
        entry.insert("unpackedSize".into(), Value::from(size));
    }
    if let Some(count) = found.inspection.file_count {
        entry.insert("fileCount".into(), Value::from(count));
    }
    if let Some(bin) = found.tar.manifest
        .as_ref()
        .and_then(|manifest| manifest.get("bin"))
    {
        entry.insert("bin".into(), bin.clone());
    }
    Ok(Value::Object(entry))
}

fn check_invariants(
    version: &str,
    variant: &VariantDeclaration,
    primary: &Value,
    published: &PublishedVariant,
) -> Result<(), RegistryError> {
    let tag = &variant.tag;
    let reject = |reason: String| bad_request(format!("variant {tag:?} of {version}: {reason}"));
    let Some(manifest) = published.tar.manifest.as_ref() else {
        return Err(reject("its package.json cannot be read".into()));
    };
    if let Some(reason) = primary_difference(manifest, primary) {
        return Err(reject(reason));
    }
    if published.inspection.requires_build != Some(false) {
        return Err(reject("it requires a build".into()));
    }
    if let Some(reason) = excluded_platform(manifest, &variant.platforms) {
        return Err(reject(reason));
    }
    Ok(())
}

/// How a variant's manifest differs from its primary's in a field the two
/// must share.
fn primary_difference(manifest: &Value, primary: &Value) -> Option<String> {
    if let Some(field) = ["name", "version"]
        .into_iter()
        .find(|field| manifest.get(field) != primary.get(field))
    {
        return Some(format!("its {field:?} differs from the primary's"));
    }
    DEPENDENCY_FIELDS
        .iter()
        .find(|field| without_bundled(manifest, field) != without_bundled(primary, field))
        .map(|field| format!("its {field:?} differ from the primary's"))
}

/// A platform value a variant declares that its manifest's own platform
/// field excludes.
fn excluded_platform(manifest: &Value, platforms: &[Value]) -> Option<String> {
    for platform in platforms {
        for field in PLATFORM_FIELDS {
            let Some(value) = platform.get(*field).and_then(Value::as_str) else { continue };
            if let Some(Value::Array(allowed)) = manifest.get(*field)
                && !allowed
                    .iter()
                    .any(|allowed| allowed.as_str() == Some(value))
            {
                return Some(format!("its package.json {field:?} excludes {value:?}"));
            }
        }
    }
    None
}

/// A dependency field, without the names the manifest bundles.
fn without_bundled(manifest: &Value, field: &str) -> Map<String, Value> {
    let bundled: HashSet<&str> = ["bundleDependencies", "bundledDependencies"]
        .iter()
        .filter_map(|key| manifest.get(*key))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    manifest
        .get(field)
        .and_then(Value::as_object)
        .map(|deps| {
            deps.iter()
                .filter(|(name, _)| !bundled.contains(name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}
