//! An npm packument taken apart into the rows that store it.
//!
//! The package-level fields keep their order: `dist-tags`, `versions`, and
//! `time` stay in [`PackumentRows::state`] as placeholders that mark where
//! [`super::render`] puts them back.

use crate::metadata::documents::check_format_version;
use pnpr_error::{RegistryError, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) const TAGS: &str = "dist-tags";
pub(super) const VERSIONS: &str = "versions";
pub(super) const TIME: &str = "time";
pub(super) const DEPRECATED: &str = "deprecated";
pub(super) const README: &str = "readme";
pub(super) const README_FILENAME: &str = "readmeFilename";

/// The revision key of a version outside the registry revision protocol.
pub(super) const ORDINARY: &str = "ordinary";
/// The interpretation profile of a manifest as the publish request stated it.
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
    /// The manifest without its README and deprecation.
    pub(super) manifest: Map<String, Value>,
    pub(super) published_at: Option<String>,
    pub(super) readme: Readme,
    pub(super) deprecated: Option<Value>,
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
            Value::Object(manifest) => Ok((version, version_rows(manifest))),
            _ => Err(bad_request(&format!("version {version:?} must be an object"))),
        })
        .collect()
}

fn version_rows(mut manifest: Map<String, Value>) -> VersionRows {
    let deprecated = manifest.shift_remove(DEPRECATED);
    let readme = Readme {
        text: manifest.shift_remove(README),
        filename: manifest.shift_remove(README_FILENAME),
    };
    VersionRows { manifest, published_at: None, readme, deprecated }
}

fn bad_request(reason: &str) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}
