//! `publishConfig.variants`: the archives of one package version
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#publishing-with-pnpm)).
//!
//! The declaration is an array with one entry per archive. An entry lists the
//! platform tags its archive runs on as `targets`, or targets `any` for the
//! version's primary archive, which every host that no platform variant
//! matches installs. A platform variant's tag is its targets, sorted by their
//! bytes and joined with `+`.
//!
//! Each entry may claim files with `files` patterns and replace fields of the
//! base manifest with `manifest`. [`VariantDeclaration::read`] runs every
//! check the declaration allows before anything is packed;
//! [`check_archive`] runs the checks of one packed archive.

pub(crate) use export::export_entry_dependencies;
pub use packing::{
    PackedArchive, PackedArchives, archive_result_json, declared_manifests, pack_archives,
};
pub(crate) use warnings::warn_about_declaration;

mod packing;

mod export;

#[cfg(test)]
mod tests;
mod warnings;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_platform_variants::{MAX_VARIANT_TAG_BYTES, Platform, canonical_spelling, variant_tag};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// The target of the entry that describes the version's primary archive.
pub const ANY_TARGET: &str = "any";

/// The longest archive file name common file systems allow, in bytes.
pub const MAX_FILE_NAME_BYTES: usize = 255;

/// Fields an entry's `manifest` may not set: every archive shares the first
/// four, pnpm writes `variant` from the targets, and `publishConfig` holds
/// the declaration.
const SHARED_FIELDS: &[&str] =
    &["name", "version", "peerDependencies", "peerDependenciesMeta", "variant", "publishConfig"];

/// Fields a platform variant may not declare: its `variant` field is its
/// one statement of where it runs.
const PLATFORM_FIELDS: &[&str] = &["os", "cpu", "libc"];

/// Fields every archive's manifest must equal the base manifest's in.
const PEER_FIELDS: &[&str] = &["peerDependencies", "peerDependenciesMeta"];

/// The parsed `publishConfig.variants` of a package.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantDeclaration {
    pub entries: Vec<VariantEntry>,
}

/// One archive of the version.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantEntry {
    /// The targets as the declaration writes them.
    pub targets: Vec<String>,
    /// The platforms of a platform variant, in tag order; `None` for the
    /// entry that targets `any`.
    pub platforms: Option<Vec<Platform>>,
    /// The derived tag of a platform variant.
    pub tag: Option<String>,
    /// Patterns, in the syntax of the `files` field, of the files only this
    /// entry's archive holds.
    pub files: Vec<String>,
    /// Fields that replace the base manifest's, `null` removing one.
    pub manifest: Map<String, Value>,
}

impl VariantEntry {
    /// Whether the entry describes the version's primary archive.
    #[must_use]
    pub fn is_any(&self) -> bool {
        self.platforms.is_none()
    }

    /// The entry as an error names it: its targets as written.
    #[must_use]
    pub fn label(&self) -> String {
        self.targets.join(", ")
    }
}

/// A `publishConfig.variants` that no archive can be packed from.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum VariantDeclarationError {
    #[display("publishConfig.variants {reason}")]
    #[diagnostic(code(ERR_PNPM_INVALID_VARIANTS))]
    Shape { reason: String },

    #[display("The variant target {target:?} is not a platform tag")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_TARGET),
        help(
            "Write it as {canonical:?}. A platform tag lists its parts in the vocabulary's order and writes glibc as no part."
        )
    )]
    NonCanonicalTarget { target: String, canonical: String },

    #[display("The variant target {target:?} is not a platform tag this pnpm knows: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_TARGET),
        help(
            r#"A platform tag is <os>-<cpu> followed by musl or a glibc floor, an OS floor, an N-API floor or a Node.js ABI, or it is "wasm32". If the tag uses a field from a later vocabulary, update pnpm."#
        )
    )]
    UnknownTarget { target: String, reason: String },

    #[display("The variant target {target:?} appears twice in publishConfig.variants")]
    #[diagnostic(code(ERR_PNPM_DUPLICATE_VARIANT_TARGET))]
    DuplicateTarget { target: String },

    #[display(
        "The entry of publishConfig.variants that targets \"any\" has other targets: {targets}"
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANTS),
        help(
            r#"The "any" entry describes the archive of hosts that no platform variant matches, so it lists no platform."#
        )
    )]
    AnyWithPlatforms { targets: String },

    #[display("The variant tag {tag:?} is {len} bytes, over the limit of {MAX_VARIANT_TAG_BYTES}")]
    #[diagnostic(
        code(ERR_PNPM_VARIANT_TAG_TOO_LONG),
        help(
            "Split the entry into entries of fewer targets. A tag is one segment of the variant's URL."
        )
    )]
    TagTooLong { tag: String, len: usize },

    #[display("The manifest of the variant for {targets} sets {field:?}")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_MANIFEST),
        help(
            "name, version, and peer dependencies are shared by every archive of the version, pnpm writes variant from the targets, and publishConfig is not published. A platform variant states where it runs only by its targets."
        )
    )]
    ForbiddenField { targets: String, field: String },

    #[display("The package manifest has {field:?}, which every platform variant would inherit")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_MANIFEST),
        help(
            r#"Remove it. To restrict where npm clients install the "any" archive, set it in that entry's manifest."#
        )
    )]
    BaseFieldInherited { field: String },

    #[display(r#"The package manifest has "variant", which pnpm writes into each archive"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_MANIFEST),
        help(
            r#"Remove it. The "any" archive has no "variant" field, and pnpm writes each platform variant's from its entry's targets."#
        )
    )]
    BaseVariant,

    #[display("No entry of publishConfig.variants targets {target:?}")]
    #[diagnostic(code(ERR_PNPM_UNKNOWN_VARIANT))]
    NoSuchEntry { target: String },
}

impl VariantDeclaration {
    /// The declaration of the package whose base manifest is `base`, or
    /// `None` when it declares no variants.
    pub fn read(base: &Value) -> Result<Option<Self>, VariantDeclarationError> {
        let Some(declared) = base.pointer("/publishConfig/variants") else { return Ok(None) };
        let Value::Array(declared) = declared else {
            return Err(shape("must be an array of entries"));
        };
        if declared.is_empty() {
            return Err(shape("must list at least one entry"));
        }
        let entries = declared
            .iter()
            .enumerate()
            .map(|(index, entry)| read_entry(index, entry))
            .collect::<Result<Vec<_>, _>>()?;
        let declaration = VariantDeclaration { entries };
        declaration.check_targets()?;
        if let Some(field) = PLATFORM_FIELDS
            .iter()
            .find(|field| base.get(**field).is_some())
        {
            return Err(VariantDeclarationError::BaseFieldInherited { field: (*field).into() });
        }
        if base.get("variant").is_some() {
            return Err(VariantDeclarationError::BaseVariant);
        }
        Ok(Some(declaration))
    }

    /// The entry that targets `any`, which a version without one lacks.
    #[must_use]
    pub fn primary(&self) -> Option<&VariantEntry> {
        self.entries.iter().find(|entry| entry.is_any())
    }

    /// The entry that `target`, any one of its targets, names.
    pub fn find(&self, target: &str) -> Result<&VariantEntry, VariantDeclarationError> {
        self.entries
            .iter()
            .find(|entry| {
                entry.targets
                    .iter()
                    .any(|own| own == target)
            })
            .ok_or_else(|| VariantDeclarationError::NoSuchEntry { target: target.into() })
    }

    /// The `package.json` of `entry`'s archive: `base` without
    /// `publishConfig.variants`, with the entry's `manifest` applied, and a
    /// platform variant's `variant` field.
    #[must_use]
    pub fn archive_manifest(base: &Value, entry: &VariantEntry) -> Value {
        let mut manifest = base_manifest(base);
        let Value::Object(fields) = &mut manifest else { return manifest };
        for (field, value) in &entry.manifest {
            if value.is_null() {
                fields.shift_remove(field);
            } else {
                fields.insert(field.clone(), value.clone());
            }
        }
        if let Some(platforms) = &entry.platforms {
            let tags: Vec<Value> = platforms
                .iter()
                .filter_map(|platform| platform.tag().ok())
                .map(Value::String)
                .collect();
            fields.insert("variant".into(), serde_json::json!({ "platforms": tags }));
        }
        manifest
    }

    /// Every target appears once across the entries. A target is a
    /// canonical tag, so two platform variants that could tie on a host,
    /// which needs two platforms of equal rank that one host matches, would
    /// list the same target.
    fn check_targets(&self) -> Result<(), VariantDeclarationError> {
        let mut seen: HashSet<&str> = HashSet::new();
        let duplicate = self.entries
            .iter()
            .flat_map(|entry| &entry.targets)
            .find(|target| !seen.insert(target));
        match duplicate {
            Some(target) => {
                Err(VariantDeclarationError::DuplicateTarget { target: target.clone() })
            }
            None => Ok(()),
        }
    }
}

/// `base` without the declaration, which no archive publishes.
pub(crate) fn base_manifest(base: &Value) -> Value {
    let mut manifest = base.clone();
    let Some(publish_config) = manifest.get_mut("publishConfig").and_then(Value::as_object_mut)
    else {
        return manifest;
    };
    publish_config.shift_remove("variants");
    if publish_config.is_empty()
        && let Value::Object(fields) = &mut manifest
    {
        fields.shift_remove("publishConfig");
    }
    manifest
}

fn shape(reason: &str) -> VariantDeclarationError {
    VariantDeclarationError::Shape { reason: reason.into() }
}

fn read_entry(index: usize, entry: &Value) -> Result<VariantEntry, VariantDeclarationError> {
    let Value::Object(entry) = entry else {
        return Err(shape(&format!("entry {index} is not an object")));
    };
    if let Some(unknown) = entry
        .keys()
        .find(|key| !matches!(key.as_str(), "targets" | "files" | "manifest"))
    {
        return Err(shape(&format!("entry {index} has an unknown field {unknown:?}")));
    }
    let targets = string_array(entry.get("targets"), index, "targets")?;
    if targets.is_empty() {
        return Err(shape(&format!("entry {index} has no targets")));
    }
    let files = match entry.get("files") {
        None => Vec::new(),
        files => string_array(files, index, "files")?,
    };
    let manifest = match entry.get("manifest") {
        None => Map::new(),
        Some(Value::Object(manifest)) => manifest.clone(),
        Some(_) => {
            return Err(shape(&format!("entry {index} has a manifest that is not an object")));
        }
    };
    let (platforms, tag) = read_targets(&targets)?;
    let forbidden = SHARED_FIELDS
        .iter()
        .chain(if platforms.is_some() { PLATFORM_FIELDS } else { &[] })
        .find(|field| manifest.contains_key(**field));
    if let Some(field) = forbidden {
        return Err(VariantDeclarationError::ForbiddenField {
            targets: targets.join(", "),
            field: (*field).into(),
        });
    }
    Ok(VariantEntry { targets, platforms, tag, files, manifest })
}

fn string_array(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<Vec<String>, VariantDeclarationError> {
    value
        .and_then(Value::as_array)
        .and_then(|values| {
            values
                .iter()
                .map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .ok_or_else(|| shape(&format!("entry {index} needs {field:?} to be an array of strings")))
}

/// The platforms and tag of an entry's targets: `None` for `any`.
fn read_targets(
    targets: &[String],
) -> Result<(Option<Vec<Platform>>, Option<String>), VariantDeclarationError> {
    if targets
        .iter()
        .any(|target| target == ANY_TARGET)
    {
        if targets.len() > 1 {
            return Err(VariantDeclarationError::AnyWithPlatforms { targets: targets.join(", ") });
        }
        return Ok((None, None));
    }
    let mut platforms = targets
        .iter()
        .map(|target| parse_target(target))
        .collect::<Result<Vec<_>, _>>()?;
    platforms.sort_by_cached_key(|platform| platform.tag().unwrap_or_default());
    let tag = variant_tag(&platforms)
        .map_err(|err| VariantDeclarationError::UnknownTarget {
            target: err.tag.clone(),
            reason: err.to_string(),
        })?;
    if tag.len() > MAX_VARIANT_TAG_BYTES {
        return Err(VariantDeclarationError::TagTooLong { len: tag.len(), tag });
    }
    Ok((Some(platforms), Some(tag)))
}

fn parse_target(target: &str) -> Result<Platform, VariantDeclarationError> {
    Platform::parse_tag(target)
        .map_err(|err| match canonical_spelling(target) {
            Some(canonical) => {
                VariantDeclarationError::NonCanonicalTarget { target: target.into(), canonical }
            }
            None => VariantDeclarationError::UnknownTarget {
                target: target.into(),
                reason: err.to_string(),
            },
        })
}

/// A packed archive that pnpr would reject.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum VariantArchiveError {
    #[display("The archive for {targets} has another {field} than the version's")]
    #[diagnostic(code(ERR_PNPM_INVALID_VARIANT_ARCHIVE))]
    Identity { targets: String, field: &'static str },

    #[display("The archive for {targets} has other {field} than the package")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_VARIANT_ARCHIVE),
        help("Every archive of a version declares the same peer dependencies.")
    )]
    Peers { targets: String, field: &'static str },

    #[display("The archive for {targets} requires a build")]
    #[diagnostic(
        code(ERR_PNPM_VARIANT_REQUIRES_BUILD),
        help(
            r#"A platform variant has an install script or a binding.gyp. Keep a source build in the "any" entry: list binding.gyp in its files and its install script in its manifest."#
        )
    )]
    RequiresBuild { targets: String },

    #[display(
        "The archive file name {file_name:?} of {package} for {targets} is {len} bytes, over the {MAX_FILE_NAME_BYTES} that file systems allow"
    )]
    #[diagnostic(
        code(ERR_PNPM_VARIANT_FILE_NAME_TOO_LONG),
        help("Split the entry into entries of fewer targets.")
    )]
    FileNameTooLong { file_name: String, package: String, targets: String, len: usize },
}

/// Check the archive of `entry`, whose `package.json` is `manifest` and
/// which holds `files`, against the package's `base` manifest, as pnpr
/// checks it.
pub fn check_archive<Filename: AsRef<str>>(
    base: &Value,
    entry: &VariantEntry,
    manifest: &Value,
    files: &[Filename],
) -> Result<(), VariantArchiveError> {
    let targets = entry.label();
    for field in ["name", "version"] {
        if manifest.get(field) != base.get(field) {
            return Err(VariantArchiveError::Identity { targets, field });
        }
    }
    if let Some(field) = PEER_FIELDS
        .iter()
        .find(|field| manifest.get(**field) != base.get(**field))
    {
        return Err(VariantArchiveError::Peers { targets, field });
    }
    let requires_build = pnpm_package_manifest::requires_build_from_files(
        Some(manifest),
        files.iter().map(AsRef::as_ref),
    );
    if !entry.is_any() && requires_build {
        return Err(VariantArchiveError::RequiresBuild { targets });
    }
    Ok(())
}

/// The file name of `entry`'s archive: `tarball_name`, the primary's, for
/// the entry that targets `any`, and `<stem>~<tag>.tgz` beside it for a
/// platform variant.
pub fn archive_file_name(
    tarball_name: &str,
    package: &str,
    entry: &VariantEntry,
) -> Result<String, VariantArchiveError> {
    let Some(tag) = &entry.tag else { return Ok(tarball_name.to_string()) };
    let stem = tarball_name.strip_suffix(".tgz").unwrap_or(tarball_name);
    let file_name = format!("{stem}~{tag}.tgz");
    if file_name.len() > MAX_FILE_NAME_BYTES {
        return Err(VariantArchiveError::FileNameTooLong {
            len: file_name.len(),
            file_name,
            package: package.into(),
            targets: entry.label(),
        });
    }
    Ok(file_name)
}
