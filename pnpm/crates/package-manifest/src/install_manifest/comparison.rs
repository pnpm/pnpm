//! Comparing a submitted manifest with the archive's.

use super::{INSTALL_MANIFEST_FIELDS, InstallManifest, NODE_GYP_REBUILD};
use serde_json::{Map, Value};

/// Fields both sides derive from the archive alone, or from fields that are
/// compared themselves, so a difference in them is never news.
const DERIVED_FIELDS: &[&str] = &["hasInstallScript", "hasShrinkwrap"];

/// How a submitted manifest differs from the archive's.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ManifestComparison {
    /// The fields that differ, as `field` or `field.key` for a field that maps
    /// keys to values. Any entry rejects the submission.
    pub mismatched: Vec<String>,
    /// The differences the profile accepts. Only one exists: a submitted
    /// `scripts.install: "node-gyp rebuild"` that the archive cannot run,
    /// because it has no `binding.gyp` or opts out with `gypfile: false`,
    /// while every other lifecycle script agrees. npm's `gypfile` publish step
    /// adds that script whenever any `*.gyp` file is in the source directory,
    /// even one the archive leaves out. The archive's verdict is what an
    /// installer acts on.
    pub tolerated: Vec<String>,
}

impl ManifestComparison {
    /// Whether the two manifests agree under the profile.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        self.mismatched.is_empty()
    }
}

pub(super) fn compare(
    submitted: &InstallManifest,
    archive: &InstallManifest,
) -> ManifestComparison {
    let mut mismatched = Vec::new();
    for &field in INSTALL_MANIFEST_FIELDS {
        if DERIVED_FIELDS.contains(&field) {
            continue;
        }
        let (submitted_value, archive_value) = (submitted.0.get(field), archive.0.get(field));
        if submitted_value == archive_value {
            continue;
        }
        match (submitted_value, archive_value) {
            (Some(Value::Object(submitted_map)), Some(Value::Object(archive_map))) => {
                differing_keys(field, submitted_map, archive_map, &mut mismatched);
            }
            _ => mismatched.push(field.to_string()),
        }
    }
    let mut comparison = ManifestComparison { mismatched, tolerated: Vec::new() };
    tolerate_unbuildable_gyp_script(&mut comparison, submitted, archive);
    comparison
}

fn differing_keys(
    field: &str,
    submitted: &Map<String, Value>,
    archive: &Map<String, Value>,
    out: &mut Vec<String>,
) {
    let mut keys: Vec<&String> = submitted
        .keys()
        .chain(archive.keys())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        if submitted.get(key) != archive.get(key) {
            out.push(format!("{field}.{key}"));
        }
    }
}

/// Move the one difference the profile accepts out of `mismatched`. It
/// applies only when every other lifecycle script agrees, so it never admits a
/// changed script body or a script the archive adds.
fn tolerate_unbuildable_gyp_script(
    comparison: &mut ManifestComparison,
    submitted: &InstallManifest,
    archive: &InstallManifest,
) {
    let script_differences: Vec<&String> = comparison.mismatched
        .iter()
        .filter(|field| field.as_str() == "scripts" || field.starts_with("scripts."))
        .collect();
    let only_install = matches!(script_differences.as_slice(), [field] if *field == "scripts.install")
        || script_differences.as_slice() == ["scripts"];
    let submitted_install = install_script(submitted);
    let archive_install = install_script(archive);
    let added_gyp = submitted_install.and_then(Value::as_str) == Some(NODE_GYP_REBUILD)
        && archive_install.is_none()
        && without_install(submitted) == without_install(archive);
    if !(only_install && added_gyp) {
        return;
    }
    comparison.mismatched.retain(|field| field != "scripts" && field != "scripts.install");
    comparison.tolerated.push("scripts.install".to_string());
}

fn install_script(manifest: &InstallManifest) -> Option<&Value> {
    manifest.0
        .get("scripts")
        .and_then(|scripts| scripts.get("install"))
}

/// The lifecycle scripts other than `install`.
fn without_install(manifest: &InstallManifest) -> Map<String, Value> {
    let mut scripts = manifest.0
        .get("scripts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    scripts.remove("install");
    scripts
}
