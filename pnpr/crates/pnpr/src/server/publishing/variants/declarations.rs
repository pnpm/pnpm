//! Reading the variants a publish body declares.

use super::{
    DECLARATION_FIELD, Declarations, RegistryError, STORED_FIELD, Value, VariantDeclaration,
    bad_request, is_valid_tag,
};
use std::collections::HashSet;

/// The fields a variant platform may set.
pub(super) const PLATFORM_FIELDS: &[&str] = &["os", "cpu", "libc"];

/// Remove variant declarations, and any server-owned variant field, from a
/// publish body, and return the declarations.
pub(in super::super) fn take_declarations(
    incoming: &mut Value,
) -> Result<Declarations, RegistryError> {
    let mut declared = Declarations::new();
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return Ok(declared);
    };
    for (version, manifest) in versions.iter_mut() {
        let Some(manifest) = manifest.as_object_mut() else { continue };
        manifest.remove(STORED_FIELD);
        let Some(list) = manifest.remove(DECLARATION_FIELD) else { continue };
        declared.insert(version.clone(), version_declarations(version, list)?);
    }
    Ok(declared)
}

/// The variants one version declares.
fn version_declarations(
    version: &str,
    list: Value,
) -> Result<Vec<VariantDeclaration>, RegistryError> {
    let Value::Array(list) = list else {
        return Err(bad_request(format!("{DECLARATION_FIELD} of {version} must be an array")));
    };
    let mut tags = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        out.push(declaration(version, &entry, &mut tags)?);
    }
    check_no_platform_overlap(version, &out)?;
    Ok(out)
}

/// One declared variant. `tags` holds the tags declared before it.
fn declaration(
    version: &str,
    entry: &Value,
    tags: &mut HashSet<String>,
) -> Result<VariantDeclaration, RegistryError> {
    let tag = entry
        .get("tag")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !is_valid_tag(&tag) {
        return Err(bad_request(format!("variant of {version} has an invalid tag {tag:?}")));
    }
    if !tags.insert(tag.clone()) {
        return Err(bad_request(format!("variant tag {tag:?} of {version} is not unique")));
    }
    let platforms = match entry.get("platforms") {
        Some(Value::Array(platforms)) if !platforms.is_empty() => platforms.clone(),
        _ => {
            return Err(bad_request(format!(
                "variant {tag:?} of {version} must list at least one platform",
            )));
        }
    };
    for platform in &platforms {
        check_platform(version, &tag, platform)?;
    }
    let Some(tgz_integrity) = entry.get("integrity").and_then(Value::as_str) else {
        return Err(bad_request(format!("variant {tag:?} of {version} has no integrity")));
    };
    Ok(VariantDeclaration { tag, platforms, tgz_integrity: tgz_integrity.to_string() })
}

fn check_platform(version: &str, tag: &str, platform: &Value) -> Result<(), RegistryError> {
    let Some(platform) = platform.as_object() else {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} is not an object",
        )));
    };
    for field in ["os", "cpu"] {
        if !platform.get(field).is_some_and(Value::is_string) {
            return Err(bad_request(format!(
                "platform of variant {tag:?} of {version} has no {field:?}",
            )));
        }
    }
    if let Some(unknown) = platform
        .keys()
        .find(|key| !PLATFORM_FIELDS.contains(&key.as_str()))
    {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} has an unknown field {unknown:?}",
        )));
    }
    if let Some(libc) = platform.get("libc")
        && !matches!(libc.as_str(), Some("glibc" | "musl"))
    {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} has an unknown libc {libc}",
        )));
    }
    Ok(())
}

/// The platform as one comparable key. A Linux platform without `libc` is
/// glibc.
fn platform_key(platform: &Value) -> (String, String, String) {
    let field = |name| {
        platform
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let os = field("os");
    let libc = match platform.get("libc").and_then(Value::as_str) {
        Some(libc) => libc.to_string(),
        None if os == "linux" => "glibc".to_string(),
        None => String::new(),
    };
    (os, field("cpu"), libc)
}

/// A [`platform_key`] as `<os>-<cpu>[-<libc>]`.
fn display_platform_key((os, cpu, libc): &(String, String, String)) -> String {
    if libc.is_empty() { format!("{os}-{cpu}") } else { format!("{os}-{cpu}-{libc}") }
}

fn check_no_platform_overlap(
    version: &str,
    variants: &[VariantDeclaration],
) -> Result<(), RegistryError> {
    let mut seen = HashSet::new();
    for variant in variants {
        for platform in &variant.platforms {
            let key = platform_key(platform);
            if !seen.insert(key.clone()) {
                return Err(bad_request(format!(
                    "more than one variant of {version} lists the platform {}",
                    display_platform_key(&key),
                )));
            }
        }
    }
    Ok(())
}
