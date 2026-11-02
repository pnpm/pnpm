//! The install-critical part of a package's manifest, under a versioned
//! interpretation profile.
//!
//! A registry derives an [`InstallManifest`] from the `package.json` inside a
//! published archive and another from the version metadata a publisher
//! submitted beside it, then [compares](InstallManifest::compare) the two. An
//! installer can apply the same derivation to the archive it extracted and the
//! metadata it resolved from. Both sides go through the same normalizations, so
//! what a publishing client rewrites on the way (a `bin` string expanded into
//! an object, `directories.bin` expanded into `bin`, an empty map dropped)
//! never reads as a disagreement, while a dependency or lifecycle script that
//! only one side declares always does.
//!
//! [`INSTALL_MANIFEST_PROFILE`] names the rules. A change to what is derived
//! or how it compares is a new profile, never an edit to this one: a manifest
//! accepted under one profile keeps the meaning it was accepted with.

pub use comparison::ManifestComparison;

mod bin_field;
mod comparison;
mod fields;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The name of the interpretation profile this module implements.
pub const INSTALL_MANIFEST_PROFILE: &str = "npm-install-manifest/1";

/// The `scripts.install` value npm writes for a package that ships a
/// `binding.gyp` and declares neither `install` nor `preinstall`.
pub const NODE_GYP_REBUILD: &str = "node-gyp rebuild";

/// What the archive itself says, beyond its `package.json`. Both sides of a
/// comparison describe the same archive, so both are derived with the same
/// facts.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveFacts<'files> {
    /// The archive has a `binding.gyp` at its root.
    pub binding_gyp: bool,
    /// The archive has an `npm-shrinkwrap.json` at its root.
    pub has_shrinkwrap: bool,
    /// Every regular file in the archive, as a package-relative path with
    /// `/` separators. Read to expand `directories.bin`.
    pub files: &'files BTreeSet<String>,
}

/// The install-critical fields of one manifest, normalized under
/// [`INSTALL_MANIFEST_PROFILE`]. Keys are the npm field names. A field the
/// profile treats as absent (an empty map, an `engines` that only says
/// `node: "*"`) is left out, except `hasInstallScript` and `hasShrinkwrap`,
/// which are always present.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallManifest(Map<String, Value>);

/// The fields an [`InstallManifest`] can carry, in the order it lists them.
pub const INSTALL_MANIFEST_FIELDS: &[&str] = &[
    "name",
    "version",
    "dependencies",
    "optionalDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "bundleDependencies",
    "os",
    "cpu",
    "libc",
    "engines",
    "bin",
    "scripts",
    "hasInstallScript",
    "hasShrinkwrap",
];

/// The version-metadata fields [`InstallManifest::write_to_version`] owns
/// beyond the profile's own: aliases that would otherwise contradict it.
const REPLACED_ALIASES: &[&str] = &["bundledDependencies", "_hasShrinkwrap"];

impl InstallManifest {
    /// Derive the install-critical fields of `manifest`, a `package.json` or
    /// one version of a packument, for the archive `archive` describes.
    #[must_use]
    pub fn derive(manifest: &Value, archive: ArchiveFacts<'_>) -> Self {
        let mut fields = Map::new();
        fields::insert_identity(&mut fields, manifest);
        fields::insert_dependencies(&mut fields, manifest);
        fields::insert_platform(&mut fields, manifest);
        if let Some(bin) = bin_field::derive_bin(manifest, archive.files) {
            fields.insert("bin".to_string(), bin);
        }
        let scripts = fields::lifecycle_scripts(manifest, archive.binding_gyp);
        let has_install_script = !scripts.is_empty();
        if has_install_script {
            fields.insert("scripts".to_string(), Value::Object(scripts));
        }
        fields.insert("hasInstallScript".to_string(), Value::Bool(has_install_script));
        fields.insert("hasShrinkwrap".to_string(), Value::Bool(archive.has_shrinkwrap));
        Self(fields)
    }

    /// The derived fields, keyed by npm field name.
    #[must_use]
    pub fn fields(&self) -> &Map<String, Value> {
        &self.0
    }

    /// Whether running the package's install runs a lifecycle script.
    #[must_use]
    pub fn has_install_script(&self) -> bool {
        self.0.get("hasInstallScript") == Some(&Value::Bool(true))
    }

    /// Compare the manifest a publisher submitted (`self`) with the one
    /// derived from the archive.
    #[must_use]
    pub fn compare(&self, archive: &Self) -> ManifestComparison {
        comparison::compare(self, archive)
    }

    /// Make `version`, one version of a packument, state these fields: each
    /// install-critical field is replaced with the derived one, or removed when
    /// the derived manifest leaves it out. `name`, `version`, and every field
    /// outside the profile are kept. `hasInstallScript` and `_hasShrinkwrap`
    /// are always written.
    pub fn write_to_version(&self, version: &mut Map<String, Value>) {
        for &field in &INSTALL_MANIFEST_FIELDS[2..] {
            if matches!(field, "scripts" | "hasInstallScript" | "hasShrinkwrap") {
                continue;
            }
            match self.0.get(field) {
                Some(value) => version.insert(field.to_string(), value.clone()),
                None => version.remove(field),
            };
        }
        for alias in REPLACED_ALIASES {
            version.remove(*alias);
        }
        fields::write_lifecycle_scripts(version, self.0.get("scripts"));
        version.insert("hasInstallScript".to_string(), Value::Bool(self.has_install_script()));
        let has_shrinkwrap = self.0.get("hasShrinkwrap").cloned();
        version.insert("_hasShrinkwrap".to_string(), has_shrinkwrap.unwrap_or(Value::Bool(false)));
    }
}

#[cfg(test)]
mod tests;
