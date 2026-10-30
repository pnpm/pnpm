//! Platform variants (proof of concept for the pnpr platform-variants RFC).
//!
//! A version publishes its primary `.tgz` plus one `.tgz` per variant,
//! attached as `<basename>-<version>~<tag>.tgz`. The publish body declares
//! each variant under `versions[<version>].pnprVariants` as
//! `{ "tag", "platforms": [{ "os", "cpu", "libc"? }], "integrity" }`, where
//! `integrity` is the `.tgz` digest the upload is verified against. The
//! registry checks the variants against the primary, records what it derived
//! under the server-owned `_pnprVariants`, and serves it as
//! `dist.pnpr.variants`.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;

use serde_json::{Map, Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;

/// The field a publish body declares variants in.
const DECLARATION_FIELD: &str = "pnprVariants";
/// The server-owned field the stored packument keeps verified variants in.
pub(in super::super) const STORED_FIELD: &str = "_pnprVariants";

/// Dependency fields every variant must share with the primary.
const DEPENDENCY_FIELDS: &[&str] =
    &["dependencies", "optionalDependencies", "peerDependencies", "peerDependenciesMeta"];

/// One declared variant of one version.
#[derive(Debug, Clone)]
pub(in super::super) struct VariantDeclaration {
    pub(in super::super) tag: String,
    pub(in super::super) platforms: Vec<Value>,
    pub(in super::super) tgz_integrity: String,
}

/// Declared variants, by version.
pub(in super::super) type Declarations = BTreeMap<String, Vec<VariantDeclaration>>;

/// What publishing a variant's `.tgz` derived from it.
pub(in super::super) struct VariantTar {
    pub(in super::super) tag: String,
    pub(in super::super) tar_integrity: String,
    pub(in super::super) manifest: Option<Value>,
}

fn bad_request(reason: String) -> RegistryError {
    RegistryError::BadRequest { reason }
}

/// Whether `tag` is a valid variant tag: lowercase letters, digits, `.`,
/// `_`, and `-`.
pub(in super::super) fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 64
        && tag
            .bytes()
            .all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
}

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
        let Value::Array(list) = list else {
            return Err(bad_request(format!("{DECLARATION_FIELD} of {version} must be an array")));
        };
        let mut tags = HashSet::new();
        let mut out = Vec::with_capacity(list.len());
        for entry in list {
            let tag = entry
                .get("tag")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if !is_valid_tag(&tag) {
                return Err(bad_request(format!(
                    "variant of {version} has an invalid tag {tag:?}"
                )));
            }
            if !tags.insert(tag.clone()) {
                return Err(bad_request(format!("variant tag {tag:?} of {version} is not unique")));
            }
            let platforms = match entry.get("platforms") {
                Some(Value::Array(platforms)) if !platforms.is_empty() => platforms.clone(),
                _ => {
                    return Err(bad_request(format!(
                        "variant {tag:?} of {version} must list at least one platform"
                    )));
                }
            };
            for platform in &platforms {
                check_platform(version, &tag, platform)?;
            }
            let Some(tgz_integrity) = entry.get("integrity").and_then(Value::as_str) else {
                return Err(bad_request(format!("variant {tag:?} of {version} has no integrity")));
            };
            out.push(VariantDeclaration {
                tag,
                platforms,
                tgz_integrity: tgz_integrity.to_string(),
            });
        }
        check_no_platform_overlap(version, &out)?;
        declared.insert(version.clone(), out);
    }
    Ok(declared)
}

const PLATFORM_FIELDS: &[&str] = &["os", "cpu", "libc"];

fn check_platform(version: &str, tag: &str, platform: &Value) -> Result<(), RegistryError> {
    let Some(platform) = platform.as_object() else {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} is not an object"
        )));
    };
    for field in ["os", "cpu"] {
        if !platform.get(field).is_some_and(Value::is_string) {
            return Err(bad_request(format!(
                "platform of variant {tag:?} of {version} has no {field:?}"
            )));
        }
    }
    if let Some(unknown) = platform
        .keys()
        .find(|key| !PLATFORM_FIELDS.contains(&key.as_str()))
    {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} has an unknown field {unknown:?}"
        )));
    }
    if let Some(libc) = platform.get("libc")
        && !matches!(libc.as_str(), Some("glibc" | "musl"))
    {
        return Err(bad_request(format!(
            "platform of variant {tag:?} of {version} has an unknown libc {libc}"
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
                    "more than one variant of {version} lists the platform {}-{}{}",
                    key.0,
                    key.1,
                    if key.2.is_empty() { String::new() } else { format!("-{}", key.2) },
                )));
            }
        }
    }
    Ok(())
}

/// Parse a variant attachment filename, `<basename>-<version>~<tag>.tgz`, into
/// its canonical blob name, version, and tag. `None` when `filename` is not a
/// variant attachment.
pub(in super::super) fn parse_variant_attachment(
    name: &CanonicalPackageName,
    filename: &str,
) -> Option<Result<(String, String, String), RegistryError>> {
    let stem = filename.strip_suffix(".tgz")?;
    let (primary_stem, tag) = stem.rsplit_once('~')?;
    if !is_valid_tag(tag) {
        return Some(Err(bad_request(format!("invalid variant tag in {filename:?}"))));
    }
    Some(
        name.parse_tarball_name(&format!("{primary_stem}.tgz"))
            .map(|(_, version)| (variant_blob_name(name, &version, tag), version, tag.to_string())),
    )
}

/// The blob a hosted variant's `.tgz` is stored in.
pub(in super::super) fn variant_blob_name(
    name: &CanonicalPackageName,
    version: &str,
    tag: &str,
) -> String {
    let primary = name.tarball_name_for_version(version);
    let stem = primary.strip_suffix(".tgz").unwrap_or(&primary);
    format!("{stem}~{tag}.tgz")
}

/// The `dist` a variant attachment is verified against.
pub(in super::super) fn attachment_dist(
    declared: &Declarations,
    version: &str,
    tag: &str,
) -> Result<Value, RegistryError> {
    declared
        .get(version)
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant.tag == tag)
        })
        .map(|variant| json!({ "integrity": variant.tgz_integrity }))
        .ok_or_else(|| {
            bad_request(format!("attachment for variant {tag:?} of {version} is not declared"))
        })
}

/// Read `package.json` from a `.tgz`.
pub(in super::super) fn read_tgz_manifest(path: &std::path::Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = flate2::read::MultiGzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.into_owned();
        let mut components = path.components();
        components.next();
        if components.as_path() == std::path::Path::new("package.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).ok()?;
            return serde_json::from_slice(&bytes).ok();
        }
    }
    None
}

/// What a published variant contributes to the stored version.
pub(in super::super) struct PublishedVariant {
    pub(in super::super) version: String,
    pub(in super::super) tar: VariantTar,
    pub(in super::super) inspection: pnpm_tarball::TarInspection,
}

/// Check every published variant against its primary and record the verified
/// variants on the merged document.
pub(in super::super) fn attach_variants(
    merged: &mut Value,
    declared: &Declarations,
    published: &[PublishedVariant],
) -> Result<(), RegistryError> {
    for (version, variants) in declared {
        let primary = merged
            .pointer(&format!("/versions/{}", escape_pointer(version)))
            .cloned()
            .ok_or_else(|| {
                bad_request(format!("variants declared for unknown version {version}"))
            })?;
        let mut stored = Vec::with_capacity(variants.len());
        for variant in variants {
            let Some(found) = published
                .iter()
                .find(|item| item.version == *version && item.tar.tag == variant.tag)
            else {
                return Err(bad_request(format!(
                    "variant {:?} of {version} is declared but not attached",
                    variant.tag
                )));
            };
            check_invariants(version, variant, &primary, found)?;
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
            stored.push(Value::Object(entry));
        }
        if let Some(manifest) = merged
            .pointer_mut(&format!("/versions/{}", escape_pointer(version)))
            .and_then(Value::as_object_mut)
        {
            manifest.insert(STORED_FIELD.to_string(), Value::Array(stored));
        }
    }
    Ok(())
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
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
    for field in ["name", "version"] {
        if manifest.get(field) != primary.get(field) {
            return Err(reject(format!("its {field:?} differs from the primary's")));
        }
    }
    for field in DEPENDENCY_FIELDS {
        let variant_deps = without_bundled(manifest, field);
        let primary_deps = without_bundled(primary, field);
        if variant_deps != primary_deps {
            return Err(reject(format!("its {field:?} differ from the primary's")));
        }
    }
    if published.inspection.requires_build != Some(false) {
        return Err(reject("it requires a build".into()));
    }
    for platform in &variant.platforms {
        for field in PLATFORM_FIELDS {
            let Some(value) = platform.get(*field).and_then(Value::as_str) else { continue };
            if let Some(Value::Array(allowed)) = manifest.get(*field)
                && !allowed
                    .iter()
                    .any(|allowed| allowed.as_str() == Some(value))
            {
                return Err(reject(format!("its package.json {field:?} excludes {value:?}")));
            }
        }
    }
    Ok(())
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

/// Move each version's stored variants into `dist.pnpr.variants`, with the
/// route each is served at.
pub fn describe_variants(doc: &mut Value, name: &str, public_url: &str) {
    let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for (version, manifest) in versions.iter_mut() {
        describe_version_variants(manifest, name, version, public_url);
    }
}

/// [`describe_variants`] for one version manifest.
pub fn describe_version_variants(
    manifest: &mut Value,
    name: &str,
    version: &str,
    public_url: &str,
) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    let Some(Value::Array(mut variants)) = manifest.remove(STORED_FIELD) else { return };
    let public_url = public_url.trim_end_matches('/');
    for variant in &mut variants {
        let Some(variant) = variant.as_object_mut() else { continue };
        let tag = variant
            .get("tag")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        variant.insert(
            "tarball".into(),
            Value::String(format!("{public_url}/{name}@{version}/variants/{tag}")),
        );
    }
    let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) else { return };
    let pnpr = dist
        .entry("pnpr")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(pnpr) = pnpr.as_object_mut() {
        pnpr.insert("variants".into(), Value::Array(variants));
    }
}
