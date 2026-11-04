//! Checking published variants against their version, and deriving what
//! pnpr's document says about each one.

use super::{
    Declarations, GENERATED_ARCHIVE_FIELD, Map, PublishedTar, PublishedVariant, RegistryError,
    STORED_FIELD, Value, VariantDeclaration, bad_request,
};
use pnpm_package_manifest::{ArchiveFacts, InstallManifest};
use pnpm_platform_variants::{Platform, variant_tag};
use pnpr_package_name::CanonicalPackageName;
use std::collections::{BTreeSet, HashMap};

/// Fields every archive of a version must share, so the package's peers, and
/// with them its key in the lockfile, do not depend on the platform.
const SHARED_FIELDS: &[&str] = &["peerDependencies", "peerDependenciesMeta"];

/// Platform fields a variant must not declare: its `variant` field is its one
/// statement of where it runs.
const PLATFORM_FIELDS: &[&str] = &["os", "cpu", "libc"];

/// Fields of an archive's manifest a variant entry does not compare.
const IDENTITY_FIELDS: &[&str] = &["name", "version", "deprecated", "_npmUser", "dist", "variant"];

/// Check every published variant against its version and record the verified
/// variants on the merged document. A version in `without_any` has no
/// `any` archive: its variants are checked against its submitted manifest
/// and described against the manifest they share, which is recorded with it
/// (see [`GENERATED_ARCHIVE_FIELD`]).
pub(in super::super) fn attach_variants(
    (name, merged): (&CanonicalPackageName, &mut Value),
    declared: &Declarations,
    (published, any_archives): (&[PublishedVariant], &[PublishedTar]),
    without_any: &BTreeSet<String>,
) -> Result<(), RegistryError> {
    for (version, variants) in declared {
        let pointer = format!("/versions/{}", escape_pointer(version));
        let submitted = merged
            .pointer(&pointer)
            .cloned()
            .ok_or_else(|| {
                bad_request(format!("variants declared for unknown version {version}"))
            })?;
        let name = name.as_str();
        let (submitted, shared) = version_manifests(
            (name, version),
            submitted,
            without_any.contains(version),
            (published, any_archives),
        )?;
        let described = shared.as_ref().unwrap_or(&submitted);
        let stored =
            stored_variants((name, version), variants, (&submitted, described), published)?;
        if let Some(manifest) = merged.pointer_mut(&pointer).and_then(Value::as_object_mut) {
            manifest.insert(STORED_FIELD.to_string(), Value::Array(stored));
            if let Some(Value::Object(mut shared)) = shared {
                shared.remove("name");
                shared.remove("version");
                manifest.insert(GENERATED_ARCHIVE_FIELD.to_string(), Value::Object(shared));
            }
        }
    }
    Ok(())
}

/// The manifest `version`'s variants are checked against, and for a version
/// published `without_any_archive`, the manifest they share. Such a version
/// was not normalized by an archive's check, so it is here, as its variants'
/// manifests are, and must name the published package and version itself;
/// any other version is checked against its `any` archive.
fn version_manifests(
    (name, version): (&str, &str),
    submitted: Value,
    without_any_archive: bool,
    (published, any_archives): (&[PublishedVariant], &[PublishedTar]),
) -> Result<(Value, Option<Value>), RegistryError> {
    if !without_any_archive {
        check_any_archive((name, version), &submitted, any_archives)?;
        return Ok((submitted, None));
    }
    if submitted.get("variant").is_some() {
        return Err(bad_request(format!(
            "version {version} declares a variant: it is the version, not one of its variants",
        )));
    }
    let submitted = normalized(&submitted);
    check_identity((name, version), &submitted)
        .map_err(|field| bad_request(format!("version {version} names another {field}")))?;
    let shared = shared_manifest(&submitted, version, published);
    Ok((submitted, Some(shared)))
}

/// `submitted`, a version's manifest, normalized as an archive's manifest is
/// (see [`InstallManifest`]), so it compares with its variants' manifests.
fn normalized(submitted: &Value) -> Value {
    let Value::Object(mut fields) = submitted.clone() else { return submitted.clone() };
    let files = BTreeSet::new();
    let facts = ArchiveFacts { binding_gyp: false, has_shrinkwrap: false, files: &files };
    InstallManifest::derive(submitted, facts).write_to_version(&mut fields);
    Value::Object(fields)
}

/// Check that `manifest` names the published package and the version it is
/// published as, or give the field that names another. Its version is the
/// normalized one, without build metadata, so a version key with build
/// metadata, such as the `+r<N>` pnpm reserves for registry revisions, never
/// matches.
fn check_identity((name, version): (&str, &str), manifest: &Value) -> Result<(), &'static str> {
    match [("name", name), ("version", version)]
        .into_iter()
        .find(|(field, expected)| manifest.get(field).and_then(Value::as_str) != Some(expected))
    {
        Some((field, _)) => Err(field),
        None => Ok(()),
    }
}

/// The manifest a version without an `any` archive is described by in
/// pnpr's document: its `name`, `version`, and peer dependencies, which
/// every archive shares, and each dependency every variant declares with the
/// same specifier.
fn shared_manifest(submitted: &Value, version: &str, published: &[PublishedVariant]) -> Value {
    let manifests: Vec<&Value> = published
        .iter()
        .filter(|variant| variant.version == version)
        .filter_map(|variant| variant.tar.manifest.as_ref())
        .collect();
    let mut shared = Map::new();
    for field in ["name", "version"].iter().chain(SHARED_FIELDS) {
        if let Some(value) = submitted.get(*field) {
            shared.insert((*field).to_string(), value.clone());
        }
    }
    for field in ["dependencies", "optionalDependencies"] {
        let Some((first, rest)) = manifests.split_first() else { continue };
        let Some(Value::Object(candidates)) = first.get(field) else { continue };
        let common: Map<String, Value> = candidates
            .iter()
            .filter(|(dependency, spec)| {
                rest.iter()
                    .all(|manifest| {
                        manifest.pointer(&format!("/{field}/{}", escape_pointer(dependency)))
                            == Some(*spec)
                    })
            })
            .map(|(dependency, spec)| (dependency.clone(), spec.clone()))
            .collect();
        if !common.is_empty() {
            shared.insert(field.to_string(), Value::Object(common));
        }
    }
    Value::Object(shared)
}

/// Check the `any` archive a version's variants are published with. The
/// variant set is part of the immutable version, so variants come only with
/// their version's `any` archive, never for a version already hosted. The
/// archive's own manifest, which clients install, must name the published
/// package and version, agree with the version's on what every archive
/// shares, and declare no variant.
fn check_any_archive(
    (name, version): (&str, &str),
    submitted: &Value,
    any_archives: &[PublishedTar],
) -> Result<(), RegistryError> {
    let Some(archive) = any_archives
        .iter()
        .find(|tar| tar.version == version)
    else {
        return Err(bad_request(format!(
            "variants of {version} are published only with its `any` archive",
        )));
    };
    let manifest = pnpm_package_manifest::parse_manifest_bytes(&archive.archive.manifest)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| {
            bad_request(format!("the `any` archive of {version} has no package.json"))
        })?;
    let manifest = &manifest;
    check_identity((name, version), manifest)
        .map_err(|field| {
            bad_request(format!(
                "the `any` archive of {version} names another {field} than the published one",
            ))
        })?;
    if submitted.get("variant").is_some() || manifest.get("variant").is_some() {
        return Err(bad_request(format!("the `any` archive of {version} declares a variant")));
    }
    match shared_difference(manifest, submitted) {
        Some(reason) => Err(bad_request(format!("the `any` archive of {version}: {reason}"))),
        None => Ok(()),
    }
}

/// What the stored version keeps of each of its verified variants. Two
/// variants may not list the same platform.
fn stored_variants(
    (name, version): (&str, &str),
    variants: &[VariantDeclaration],
    submitted: (&Value, &Value),
    published: &[PublishedVariant],
) -> Result<Vec<Value>, RegistryError> {
    let mut stored = Vec::with_capacity(variants.len());
    let mut listed: HashMap<Platform, &str> = HashMap::new();
    for variant in variants {
        let (entry, platforms) = stored_variant((name, version), variant, submitted, published)?;
        let overlap = platforms
            .into_iter()
            .find_map(|platform| {
                let other = listed.insert(platform.clone(), &variant.tag)?;
                Some((other, platform))
            });
        if let Some((other, platform)) = overlap {
            return Err(bad_request(format!(
                "variants {other:?} and {:?} of {version} both list {}",
                variant.tag,
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
/// The variant is checked against the first manifest of `submitted`, the
/// version's, and
/// described by its difference from the second.
fn stored_variant(
    (name, version): (&str, &str),
    variant: &VariantDeclaration,
    (submitted, described): (&Value, &Value),
    published: &[PublishedVariant],
) -> Result<(Value, Vec<Platform>), RegistryError> {
    let named = &variant.tag;
    let reject = |reason: String| bad_request(format!("variant {named:?} of {version}: {reason}"));
    let Some(found) = published
        .iter()
        .find(|item| item.version == *version && item.tar.tag == *named)
    else {
        return Err(bad_request(format!("variant {named:?} of {version} has no archive")));
    };
    let manifest = verified_manifest(found, submitted).map_err(reject)?;
    check_identity((name, version), manifest)
        .map_err(|field| reject(format!("it names another {field} than the published one")))?;
    let platforms = declared_platforms(manifest).map_err(reject)?;
    let tag = variant_tag(&platforms).map_err(|err| reject(err.to_string()))?;
    if tag != *named {
        return Err(reject(format!("its variant field gives the tag {tag:?}")));
    }
    let mut entry = archive_facts(found);
    entry.insert("tag".into(), Value::String(tag));
    entry.insert(
        "platforms".into(),
        serde_json::to_value(&platforms).expect("platforms serialize"),
    );
    entry.insert("manifest".into(), Value::Object(manifest_difference(manifest, described)));
    Ok((Value::Object(entry), platforms))
}

/// The variant archive's manifest, once it passes the invariants a variant
/// shares with its version.
fn verified_manifest<'found>(
    found: &'found PublishedVariant,
    submitted: &Value,
) -> Result<&'found Value, String> {
    let manifest = found.tar.manifest.as_ref().ok_or("its package.json cannot be read")?;
    if let Some(reason) = shared_difference(manifest, submitted) {
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
fn shared_difference(manifest: &Value, submitted: &Value) -> Option<String> {
    ["name", "version"]
        .into_iter()
        .chain(SHARED_FIELDS.iter().copied())
        .find(|field| manifest.get(field) != submitted.get(field))
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
fn manifest_difference(manifest: &Value, submitted: &Value) -> Map<String, Value> {
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
    let version = fields(submitted);
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
