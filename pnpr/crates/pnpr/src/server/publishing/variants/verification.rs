//! Checking published variants against their version, and deriving what
//! pnpr's document says about each one.

use super::{
    Declarations, LABEL_FIELD, Map, PublishedVariant, RegistryError, STORED_FIELD, Value,
    VariantDeclaration, bad_request,
};
use pnpm_platform_variants::{Platform, variant_tag};
use std::collections::HashMap;

/// Fields every archive of a version must share, so the package's peers, and
/// with them its key in the lockfile, do not depend on the platform.
const SHARED_FIELDS: &[&str] = &["peerDependencies", "peerDependenciesMeta"];

/// Platform fields a variant must not declare: its `variant` field is its one
/// statement of where it runs.
const PLATFORM_FIELDS: &[&str] = &["os", "cpu", "libc"];

/// Fields of an archive's manifest a variant entry does not compare.
const IDENTITY_FIELDS: &[&str] = &["name", "version", "deprecated", "_npmUser", "dist", "variant"];

/// Check every published variant against its version and record the verified
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
        if primary.get("variant").is_some() {
            return Err(bad_request(format!(
                "the primary archive of {version} declares a variant",
            )));
        }
        let stored = stored_variants(version, variants, &primary, published)?;
        if let Some(manifest) = merged.pointer_mut(&pointer).and_then(Value::as_object_mut) {
            manifest.insert(STORED_FIELD.to_string(), Value::Array(stored));
        }
    }
    Ok(())
}

/// What the stored version keeps of each of its verified variants. Two
/// variants may not list the same platform.
fn stored_variants(
    version: &str,
    variants: &[VariantDeclaration],
    primary: &Value,
    published: &[PublishedVariant],
) -> Result<Vec<Value>, RegistryError> {
    let mut stored = Vec::with_capacity(variants.len());
    let mut listed: HashMap<Platform, &str> = HashMap::new();
    for variant in variants {
        let (entry, platforms) = stored_variant(version, variant, primary, published)?;
        let overlap = platforms
            .into_iter()
            .find_map(|platform| {
                let other = listed.insert(platform.clone(), &variant.label)?;
                Some((other, platform))
            });
        if let Some((other, platform)) = overlap {
            return Err(bad_request(format!(
                "variants {other:?} and {:?} of {version} both list {}",
                variant.label,
                platform.tag().unwrap_or_default(),
            )));
        }
        stored.push(entry);
    }
    Ok(stored)
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// What the stored version keeps of one verified variant, and its platforms.
fn stored_variant(
    version: &str,
    variant: &VariantDeclaration,
    primary: &Value,
    published: &[PublishedVariant],
) -> Result<(Value, Vec<Platform>), RegistryError> {
    let label = &variant.label;
    let reject = |reason: String| bad_request(format!("variant {label:?} of {version}: {reason}"));
    let Some(found) = published
        .iter()
        .find(|item| item.version == *version && item.tar.label == *label)
    else {
        return Err(bad_request(format!(
            "variant {label:?} of {version} is declared but not attached",
        )));
    };
    let manifest = verified_manifest(found, primary).map_err(reject)?;
    let platforms = declared_platforms(manifest).map_err(reject)?;
    let tag = variant_tag(&platforms).map_err(|err| reject(err.to_string()))?;
    let mut entry = archive_facts(found);
    entry.insert("tag".into(), Value::String(tag));
    entry.insert(
        "platforms".into(),
        serde_json::to_value(&platforms).expect("platforms serialize"),
    );
    entry.insert("manifest".into(), Value::Object(manifest_difference(manifest, primary)));
    entry.insert(LABEL_FIELD.into(), Value::String(label.clone()));
    Ok((Value::Object(entry), platforms))
}

/// The variant archive's manifest, once it passes the invariants a variant
/// shares with its version.
fn verified_manifest<'found>(
    found: &'found PublishedVariant,
    primary: &Value,
) -> Result<&'found Value, String> {
    let manifest = found.tar.manifest.as_ref().ok_or("its package.json cannot be read")?;
    if let Some(reason) = primary_difference(manifest, primary) {
        return Err(reason);
    }
    if found.inspection.requires_build != Some(false) {
        return Err("it requires a build".into());
    }
    match PLATFORM_FIELDS
        .iter()
        .find(|field| manifest.get(**field).is_some())
    {
        Some(field) => Err(format!("it declares {field:?} beside its variant field")),
        None => Ok(manifest),
    }
}

/// pnpr's facts about a variant's archive: its tar's integrity, sizes, and
/// bins.
fn archive_facts(found: &PublishedVariant) -> Map<String, Value> {
    let mut facts = Map::new();
    facts.insert("integrity".into(), Value::String(found.tar.tar_integrity.clone()));
    if let Some(size) = found.inspection.unpacked_size {
        facts.insert("unpackedSize".into(), Value::from(size));
    }
    if let Some(count) = found.inspection.file_count {
        facts.insert("fileCount".into(), Value::from(count));
    }
    if let Some((bins, _)) =
        found.inspection.bins.as_ref().and_then(pnpm_package_manifest::serialize_precalculated_bins)
    {
        facts.insert("bins".into(), bins);
    }
    facts
}

/// How a variant's manifest differs from its version's in a field the two
/// must share.
fn primary_difference(manifest: &Value, primary: &Value) -> Option<String> {
    ["name", "version"]
        .into_iter()
        .chain(SHARED_FIELDS.iter().copied())
        .find(|field| manifest.get(field) != primary.get(field))
        .map(|field| format!("its {field:?} differs from the version's"))
}

/// The platforms a variant's `variant` field declares, parsed from their
/// tags. A field that does not parse fails publication, so a `variant` field
/// that means something else is never misread.
fn declared_platforms(manifest: &Value) -> Result<Vec<Platform>, String> {
    let Some(Value::Object(variant)) = manifest.get("variant") else {
        return Err("its package.json has no variant field".into());
    };
    if let Some(unknown) = variant
        .keys()
        .find(|key| *key != "platforms")
    {
        return Err(format!("its variant field has an unknown key {unknown:?}"));
    }
    let tags = match variant.get("platforms") {
        Some(Value::Array(tags)) if !tags.is_empty() => tags,
        _ => return Err("its variant field lists no platforms".into()),
    };
    tags.iter()
        .map(|tag| {
            let tag = tag.as_str().ok_or("a platform is not a string")?;
            Platform::parse_tag(tag).map_err(|err| err.to_string())
        })
        .collect()
}

/// The fields of a variant's manifest whose value differs from its version's,
/// with `null` for a field only the version has. Each replaces the version's
/// field as a whole.
fn manifest_difference(manifest: &Value, primary: &Value) -> Map<String, Value> {
    let fields = |manifest: &Value| match pnpr_upstream::abbreviate_version(manifest) {
        Value::Object(mut fields) => {
            for field in IDENTITY_FIELDS {
                fields.remove(*field);
            }
            fields
        }
        _ => Map::new(),
    };
    let variant = fields(manifest);
    let version = fields(primary);
    let changed = variant
        .iter()
        .filter(|(field, value)| version.get(*field) != Some(*value))
        .map(|(field, value)| (field.clone(), value.clone()));
    let removed = version
        .keys()
        .filter(|field| !variant.contains_key(*field))
        .map(|field| (field.clone(), Value::Null));
    changed.chain(removed).collect()
}
