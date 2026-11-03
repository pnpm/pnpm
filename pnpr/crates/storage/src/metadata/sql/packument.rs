//! An npm packument taken apart into the rows that store it.
//!
//! The package-level fields keep their order: `dist-tags`, `versions`, and
//! `time` stay in [`PackumentRows::state`] as placeholders that mark where
//! [`super::render`] puts them back.

use crate::{metadata::ARCHIVE_MANIFEST_FIELD, publish::PACKUMENT_FORMAT_VERSION};
use pnpr_error::{RegistryError, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) const TAGS: &str = "dist-tags";
pub(super) const VERSIONS: &str = "versions";
pub(super) const TIME: &str = "time";
pub(super) const DEPRECATED: &str = "deprecated";
pub(super) const README: &str = "readme";
pub(super) const README_FILENAME: &str = "readmeFilename";
/// The key of the profile in an `_archiveManifest` record.
const PROFILE: &str = "profile";

/// The revision key of a version outside the registry revision protocol.
pub(super) const ORDINARY: &str = "ordinary";
/// The interpretation profile of a manifest as the publish request stated it,
/// for a version that records no other.
pub(super) const SUBMITTED_PROFILE: &str = "submitted";

#[derive(Debug)]
pub(super) struct PackumentRows {
    /// The package-level fields, with `time` cut down to the entries that
    /// name no version.
    pub(super) state: Map<String, Value>,
    pub(super) tags: BTreeMap<String, String>,
    pub(super) versions: BTreeMap<String, VersionRows>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct VersionRows {
    /// The manifest without its README, deprecation, and interpretation.
    pub(super) manifest: Map<String, Value>,
    pub(super) interpretation: Interpretation,
    pub(super) published_at: Option<String>,
    pub(super) readme: Readme,
    pub(super) deprecated: Option<Value>,
}

/// The interpretation profile a manifest was accepted under, and the digests
/// of the raw inputs the profile derived it from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(super) struct Interpretation {
    pub(super) profile: String,
    /// A JSON object of input digests; empty for [`SUBMITTED_PROFILE`],
    /// whose input is the manifest itself.
    pub(super) inputs: Map<String, Value>,
}

impl Interpretation {
    fn submitted() -> Self {
        Self { profile: SUBMITTED_PROFILE.to_string(), inputs: Map::new() }
    }

    /// Take the interpretation out of a version's `_archiveManifest` record.
    fn from_record(version: &str, record: Option<Value>) -> Result<Self> {
        let Some(record) = record else { return Ok(Self::submitted()) };
        let invalid = || {
            bad_request(&format!(
                "`{ARCHIVE_MANIFEST_FIELD}` of version {version:?} must be an object naming a profile",
            ))
        };
        let Value::Object(mut inputs) = record else { return Err(invalid()) };
        let Some(Value::String(profile)) = inputs.shift_remove(PROFILE) else {
            return Err(invalid());
        };
        if profile == SUBMITTED_PROFILE || !inputs.values().all(Value::is_string) {
            return Err(invalid());
        }
        Ok(Self { profile, inputs })
    }

    /// The `_archiveManifest` record a rendered version carries, `None` for
    /// [`SUBMITTED_PROFILE`].
    pub(super) fn record(&self) -> Option<Value> {
        if self.profile == SUBMITTED_PROFILE {
            return None;
        }
        let mut record = Map::new();
        record.insert(PROFILE.to_string(), Value::String(self.profile.clone()));
        record.extend(self.inputs.clone());
        Some(Value::Object(record))
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Readme {
    pub(super) text: Option<Value>,
    pub(super) filename: Option<Value>,
}

impl Readme {
    pub(super) fn is_empty(&self) -> bool {
        self.text.is_none() && self.filename.is_none()
    }
}

impl VersionRows {
    /// The archive integrity the manifest declares, empty when it declares
    /// none.
    pub(super) fn integrity(&self) -> &str {
        self.manifest
            .get("dist")
            .and_then(|dist| dist.get("integrity"))
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
}

/// Take a stored packument apart.
pub(super) fn decompose(bytes: &[u8]) -> Result<PackumentRows> {
    let packument: Value = serde_json::from_slice(bytes)?;
    check_format_version(&packument)?;
    let Value::Object(mut state) = packument else {
        return Err(bad_request("a packument must be an object"));
    };
    let tags = take_tags(&mut state)?;
    let mut versions = take_versions(&mut state)?;
    if let Some(time) = state.get_mut(TIME) {
        let time = time.as_object_mut().ok_or_else(|| bad_request("`time` must be an object"))?;
        for (version, rows) in &mut versions {
            if let Some(Value::String(published_at)) = time.get(version) {
                rows.published_at = Some(published_at.clone());
                time.shift_remove(version);
            }
        }
    }
    Ok(PackumentRows { state, tags, versions })
}

fn take_tags(state: &mut Map<String, Value>) -> Result<BTreeMap<String, String>> {
    let Some(tags) = state.get_mut(TAGS) else { return Ok(BTreeMap::new()) };
    let Value::Object(tags) = tags.take() else {
        return Err(bad_request("`dist-tags` must be an object"));
    };
    tags.into_iter()
        .map(|(tag, version)| match version {
            Value::String(version) => Ok((tag, version)),
            _ => Err(bad_request(&format!("dist-tag {tag:?} must name a version"))),
        })
        .collect()
}

fn take_versions(state: &mut Map<String, Value>) -> Result<BTreeMap<String, VersionRows>> {
    let Some(versions) = state.get_mut(VERSIONS) else { return Ok(BTreeMap::new()) };
    let Value::Object(versions) = versions.take() else {
        return Err(bad_request("`versions` must be an object"));
    };
    versions
        .into_iter()
        .map(|(version, manifest)| match manifest {
            Value::Object(manifest) => {
                let rows = version_rows(&version, manifest)?;
                Ok((version, rows))
            }
            _ => Err(bad_request(&format!("version {version:?} must be an object"))),
        })
        .collect()
}

fn version_rows(version: &str, mut manifest: Map<String, Value>) -> Result<VersionRows> {
    let interpretation =
        Interpretation::from_record(version, manifest.shift_remove(ARCHIVE_MANIFEST_FIELD))?;
    let deprecated = manifest.shift_remove(DEPRECATED);
    let readme = Readme {
        text: manifest.shift_remove(README),
        filename: manifest.shift_remove(README_FILENAME),
    };
    Ok(VersionRows { manifest, interpretation, published_at: None, readme, deprecated })
}

/// Refuse a packument written by a newer pnpr.
fn check_format_version(packument: &Value) -> Result<()> {
    let Some(format_version) = packument.get("formatVersion").and_then(Value::as_u64) else {
        return Ok(());
    };
    if format_version <= u64::from(PACKUMENT_FORMAT_VERSION) {
        return Ok(());
    }
    Err(RegistryError::DocumentFormatTooNew {
        document: "packument".to_string(),
        format_version: u32::try_from(format_version).unwrap_or(u32::MAX),
        supported_version: PACKUMENT_FORMAT_VERSION,
    })
}

fn bad_request(reason: &str) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}
