pub mod package_manager_spec;
pub use initialization::{InitAuthor, InitOptions};
pub use runtime::{
    apply_runtime_on_fail_override, convert_dependencies_to_engines_runtime,
    convert_engines_runtime_to_dependencies, engines_runtime_dependencies, is_runtime_alias,
    node_version_from_engines_runtime,
};
pub use serialization::{parse_manifest, parse_manifest_bytes, safe_read_package_json_from_dir};
pub use truthiness::is_truthy;

use std::{
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use derive_more::{Display, Error, From};
use miette::Diagnostic;
use node_semver::Range;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use strum::IntoStaticStr;
use tempfile::NamedTempFile;
mod truthiness;

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum PackageManifestError {
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    Serialization(serde_json::Error), // TODO: remove derive(From), split this variant

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    Parse {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR))]
    Io(std::io::Error), // TODO: remove derive(From), split this variant

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR))]
    Read {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("package.json file already exists")]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_JSON_EXISTS),
        help("Your current working directory already has a package.json file.")
    )]
    AlreadyExist,

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("invalid attribute: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_INVALID_ATTRIBUTE))]
    InvalidAttribute(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("No package.json was found in {_0}")]
    #[diagnostic(code(ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND))]
    NoImporterManifestFound(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Missing script: {_0:?}")]
    #[diagnostic(code(ERR_PNPM_NO_SCRIPT))]
    NoScript(#[error(not(source))] String),
}

#[derive(Debug, Clone, Copy, PartialEq, IntoStaticStr)]
pub enum DependencyGroup {
    #[strum(serialize = "dependencies")]
    Prod,
    #[strum(serialize = "devDependencies")]
    Dev,
    #[strum(serialize = "optionalDependencies")]
    Optional,
    #[strum(serialize = "peerDependencies")]
    Peer,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BundleDependencies {
    Boolean(bool),
    List(Vec<String>),
}

/// Indentation for manifests with no source file to detect it from
/// (freshly scaffolded or in-memory).
const DEFAULT_INDENT: &str = "  ";

/// Content of the `package.json` files and its path.
///
/// Carries the source file's formatting (indentation unit, final-newline
/// state) and its parsed value across the read/save round-trip, so
/// [`Self::save`] preserves the file's style and skips the write entirely
/// when nothing changed — the same contract as pnpm's project-manifest
/// reader/writer pair.
#[derive(Clone)]
pub struct PackageManifest {
    path: PathBuf,
    value: Value, // TODO: convert this into a proper struct + an array of keys order
    /// Whether a save ends the file with a newline. New and in-memory
    /// manifests get one.
    insert_final_newline: bool,
    /// One indentation level. Empty for a single-line source document,
    /// which then round-trips back to its compact form.
    indent: String,
    /// The manifest as the file currently encodes it (`devEngines` folded,
    /// dependency fields normalized), used to skip a save that wouldn't
    /// change the file. `None` when there is no file baseline (in-memory
    /// manifests), so the first save always writes.
    on_disk: Option<Value>,
}

impl InitAuthor<'_> {
    /// A part that was set to the empty string counts as unset, so an
    /// `initAuthorEmail=` in the environment renders no empty `<>`.
    fn part(part: Option<&str>) -> Option<&str> {
        part.filter(|part| !part.is_empty())
    }
}

impl fmt::Display for InitAuthor<'_> {
    /// Renders npm's `name <email> (url)` shape, omitting each part that is
    /// unset. All three unset renders the empty string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name.unwrap_or_default())?;
        if let Some(email) = Self::part(self.email) {
            write!(f, " <{email}>")?;
        }
        if let Some(url) = Self::part(self.url) {
            write!(f, " ({url})")?;
        }
        Ok(())
    }
}

impl PackageManifest {
    /// Build a manifest from an in-memory JSON value paired with the path it
    /// would live at, without touching the filesystem.
    ///
    /// Applies the same `engines.runtime` → dependency normalization the
    /// on-disk read performs, so a manifest supplied programmatically (e.g. by
    /// the Node API binding) resolves identically to one read from disk.
    /// Nothing is written; [`Self::save`] persists it if the caller wants.
    #[must_use]
    pub fn from_value(path: PathBuf, mut value: Value) -> PackageManifest {
        // A manifest must be a JSON object. This is a last-resort guard: callers
        // that accept untrusted input (the Node API binding) reject a non-object
        // manifest at their boundary for a clear error, but if any value other
        // than an object still reaches here, inserting a dependency via
        // `self.value[key] = ...` would panic — so coerce it to an empty object
        // rather than aborting the host process.
        if !value.is_object() {
            value = json!({});
        }
        convert_engines_runtime_to_dependencies(&mut value, "devEngines", "devDependencies");
        convert_engines_runtime_to_dependencies(&mut value, "engines", "dependencies");
        PackageManifest {
            path,
            value,
            insert_final_newline: true,
            indent: DEFAULT_INDENT.to_string(),
            on_disk: None,
        }
    }

    #[must_use]
    pub fn path(&self) -> &'_ Path {
        &self.path
    }

    #[must_use]
    pub fn value(&self) -> &'_ Value {
        &self.value
    }

    /// In-memory mutation handle on the underlying JSON value.
    ///
    /// Used by the read-package-hook layer to rewrite a manifest's
    /// dependency maps before downstream consumers see it (the
    /// `readPackage` hook returns a modified manifest). Mutations stay
    /// in memory — there is no implicit `save`, so the user's on-disk
    /// `package.json` is untouched.
    pub fn value_mut(&mut self) -> &'_ mut Value {
        &mut self.value
    }

    /// Return the manifest shape that [`Self::save`] would write without
    /// changing the filesystem.
    pub fn written_value(&self) -> Result<Value, PackageManifestError> {
        let mut value = self.value.clone();
        convert_dependencies_to_engines_runtime(&mut value, "devDependencies", "devEngines")?;
        convert_dependencies_to_engines_runtime(&mut value, "dependencies", "engines")?;
        normalize_dependency_fields(&mut value);
        Ok(value)
    }

    /// Persist the manifest in its on-disk shape (`devEngines` folded back,
    /// dependency fields normalized) and return that shape.
    ///
    /// The write preserves the source file's indentation and final-newline
    /// state, and is skipped entirely when the file already encodes the
    /// same manifest — so a no-op save never churns formatting or mtime.
    pub fn save_and_get_written_value(&mut self) -> Result<Value, PackageManifestError> {
        let value = self.written_value()?;
        if self.on_disk.as_ref() == Some(&value) {
            return Ok(value);
        }
        let mut contents = serialize_with_indent(&value, &self.indent)?;
        if self.insert_final_newline {
            contents.push('\n');
        }
        Self::write_atomic(&self.path, &contents)?;
        self.on_disk = Some(value.clone());
        Ok(value)
    }

    pub fn save(&mut self) -> Result<(), PackageManifestError> {
        self.save_and_get_written_value()?;
        Ok(())
    }

    pub fn dependencies<'a>(
        &'a self,
        groups: impl IntoIterator<Item = DependencyGroup> + 'a,
    ) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        // TODO: add error when `dependencies` is found to not be an object
        // TODO: add error when `version` is found to not be a string
        groups
            .into_iter()
            .filter_map(|group| self.value.get::<&str>(group.into()))
            .filter_map(|dependencies| dependencies.as_object())
            .flatten()
            .filter_map(|(name, version)| version.as_str().map(|value| (name.as_str(), value)))
    }

    /// Resolve a `(key, bare_specifier)` pair from a `package.json`
    /// dependency entry into the `(registry_name, version_range)` to send
    /// to the registry.
    ///
    /// For an ordinary entry (`"foo": "^1.2.3"`) the registry name equals
    /// the entry key. For an npm-alias entry (`"foo": "npm:bar@^1.2.3"`)
    /// the registry name is parsed from the spec and the entry key is
    /// only used as the directory name under `node_modules`. An
    /// unversioned `npm:bar` (or `npm:@scope/bar`) defaults to the
    /// `latest` tag.
    #[must_use]
    pub fn resolve_registry_dependency<'a>(
        key: &'a str,
        bare_specifier: &'a str,
    ) -> (&'a str, &'a str) {
        let Some(rest) = bare_specifier.strip_prefix("npm:") else {
            return (key, bare_specifier);
        };
        // The bare-specifier parse uses `lastIndexOf('@')` and treats
        // `index < 1` (no `@`, or `@` at position 0 of a scoped name)
        // as "no version" — the spec is just a package name.
        match rest.rfind('@') {
            Some(idx) if idx >= 1 => (&rest[..idx], &rest[idx + 1..]),
            _ => (rest, "latest"),
        }
    }

    pub fn bundle_dependencies(&self) -> Result<Option<BundleDependencies>, serde_json::Error> {
        self.value
            .get("bundleDependencies")
            .or_else(|| self.value.get("bundledDependencies"))
            .cloned()
            .map(serde_json::from_value)
            .transpose()
    }

    /// The `installConfig.hoistingLimits` value declared by this
    /// manifest, if any.
    ///
    /// pnpm reads a per-package `installConfig.hoistingLimits` to
    /// override how far that package's dependencies are hoisted; the
    /// value mirrors the workspace-wide `hoistingLimits` setting
    /// (`"dependencies" | "workspaces" | "none"`). Bit stamps
    /// `"workspaces"` on the per-root-component importer manifests it
    /// generates under `node_modules/.bit_roots/<id>`; the isolated
    /// linker keys root-component member reachability off that value.
    /// Returned verbatim so callers can match whichever mode they care
    /// about (today only `"workspaces"` is acted on).
    #[must_use]
    pub fn install_config_hoisting_limits(&self) -> Option<&str> {
        self.value
            .get("installConfig")
            .and_then(|install_config| install_config.get("hoistingLimits"))
            .and_then(Value::as_str)
    }

    /// Record `name@version` under `dependency_group`. Saving into one
    /// of the install groups (`dependencies` / `devDependencies` /
    /// `optionalDependencies`) drops `name` from the other two: a
    /// dependency has one home there, so saving it as a different type
    /// moves it, matching pnpm's `updateProjectManifestObject`. A
    /// [`DependencyGroup::Peer`] save is additive — pnpm's `--save-peer`
    /// writes `peerDependencies` alongside the `devDependencies` entry.
    pub fn add_dependency(
        &mut self,
        name: &str,
        version: &str,
        dependency_group: DependencyGroup,
    ) -> Result<(), PackageManifestError> {
        let dependency_type: &str = dependency_group.into();
        let Some(field) = self.value.get_mut(dependency_type) else {
            let mut dependencies = Map::<String, Value>::new();
            dependencies.insert(name.to_string(), Value::String(version.to_string()));
            self.value[dependency_type] = Value::Object(dependencies);
            self.drop_from_other_install_groups(name, dependency_group);
            return Ok(());
        };
        let Some(dependencies) = field.as_object_mut() else {
            return Err(PackageManifestError::InvalidAttribute(
                "dependencies attribute should be an object".to_string(),
            ));
        };
        dependencies.insert(name.to_string(), Value::String(version.to_string()));
        self.drop_from_other_install_groups(name, dependency_group);
        Ok(())
    }

    /// A dependency belongs to one install group at a time, so adding it to
    /// one removes it from the other two.
    fn drop_from_other_install_groups(&mut self, name: &str, added_to: DependencyGroup) {
        const INSTALL_GROUPS: [DependencyGroup; 3] =
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        if !INSTALL_GROUPS.contains(&added_to) {
            return;
        }
        let removed = [name.to_string()];
        for group in INSTALL_GROUPS {
            if group != added_to {
                self.remove_from_object(group.into(), &removed);
            }
        }
    }

    /// Names eligible for `pnpm remove` to target.
    ///
    /// When `save_type` is `Some`, the keys of just that field; when
    /// `None`, the union of `dependencies`, `devDependencies`, and
    /// `optionalDependencies` (peer dependencies excluded), preserving
    /// first-seen order. This is the set `pnpm remove` validates removal
    /// targets against.
    #[must_use]
    pub fn available_dependency_names(&self, save_type: Option<DependencyGroup>) -> Vec<String> {
        let groups: &[DependencyGroup] = match save_type {
            Some(ref group) => std::slice::from_ref(group),
            None => &[DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional],
        };
        let mut seen = std::collections::HashSet::new();
        self.dependencies(groups.iter().copied())
            .filter(|(name, _)| seen.insert(*name))
            .map(|(name, _)| name.to_string())
            .collect()
    }

    /// Drop `removed_packages` from the manifest's dependency maps.
    ///
    /// When `save_type` is `Some`, only that field is touched; otherwise
    /// every dependency field (`optionalDependencies`, `dependencies`,
    /// `devDependencies`) is scanned. `peerDependencies` and
    /// `dependenciesMeta` entries for the removed names are always
    /// dropped, regardless of `save_type`.
    pub fn remove_dependencies(
        &mut self,
        removed_packages: &[String],
        save_type: Option<DependencyGroup>,
    ) {
        let groups: &[DependencyGroup] = match save_type {
            Some(ref group) => std::slice::from_ref(group),
            None => &[DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev],
        };
        for group in groups {
            self.remove_from_object((*group).into(), removed_packages);
        }
        self.remove_from_object("peerDependencies", removed_packages);
        self.remove_from_object("dependenciesMeta", removed_packages);
    }

    fn remove_from_object(&mut self, key: &str, removed_packages: &[String]) {
        if let Some(object) = self.value.get_mut(key).and_then(Value::as_object_mut) {
            for name in removed_packages {
                object.remove(name);
            }
        }
    }

    pub fn script(
        &self,
        command: &str,
        if_present: bool, // TODO: split this function into 2, one with --if-present, one without
    ) -> Result<Option<&str>, PackageManifestError> {
        if let Some(script_str) = self
            .value
            .get("scripts")
            .and_then(|scripts| scripts.get(command))
            .and_then(|script| script.as_str())
        {
            return Ok(Some(script_str));
        }

        if if_present { Ok(None) } else { Err(PackageManifestError::NoScript(command.to_string())) }
    }
}

/// Decide whether a package directory needs a build pass.
///
/// True when the package's manifest declares any of `preinstall`, `install`,
/// or `postinstall`, or when the package contains `binding.gyp` or a `.hooks/`
/// directory. Missing manifests, IO errors, and parse errors all collapse to
/// `false` — pacquet cannot meaningfully build a package whose extracted
/// content cannot be inspected.
#[must_use]
pub fn pkg_requires_build(pkg_root: &Path) -> bool {
    if pkg_root.join("binding.gyp").exists() || pkg_root.join(".hooks").is_dir() {
        return true;
    }
    let Ok(Some(manifest)) = safe_read_package_json_from_dir(pkg_root) else { return false };
    manifest_requires_build(&manifest)
}

/// Decide whether a parsed manifest declares lifecycle scripts that
/// make its package a build candidate.
///
/// A script has to carry a value to count. An empty `postinstall` runs
/// nothing, and pnpm v11's `pkgRequiresBuild` reads the same manifest as
/// build-free, so treating the key's presence as build work would ask the
/// user to approve a build that does not exist.
#[must_use]
pub fn manifest_requires_build(manifest: &Value) -> bool {
    manifest.get("scripts").and_then(Value::as_object).is_some_and(|scripts| {
        ["preinstall", "install", "postinstall"]
            .iter()
            .any(|name| scripts.get(*name).is_some_and(script_is_set))
    })
}

/// Whether a `scripts` entry holds something to run.
///
/// Mirrors `Boolean(manifest.scripts.postinstall)` in pnpm v11's
/// `pkgRequiresBuild`: `null`, `false`, `0`, and `""` are the falsy values
/// a manifest can carry there.
fn script_is_set(script: &Value) -> bool {
    match script {
        Value::String(script) => !script.is_empty(),
        Value::Null | Value::Bool(false) => false,
        Value::Number(number) => number.as_f64() != Some(0.0),
        _ => true,
    }
}

/// Decide whether a store-index file key implies build hooks.
#[must_use]
pub fn file_path_requires_build(filename: &str) -> bool {
    filename == "binding.gyp"
        || filename
            .strip_prefix(".hooks")
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}

#[must_use]
pub fn files_include_install_scripts<Filenames, Filename>(filenames: Filenames) -> bool
where
    Filenames: IntoIterator<Item = Filename>,
    Filename: AsRef<str>,
{
    filenames.into_iter().any(|filename| file_path_requires_build(filename.as_ref()))
}

#[cfg(test)]
mod tests;

/// Extracts the author field from a manifest (either string or object with name).
///
/// A blank name is no name: an SBOM would otherwise carry it as the nameless
/// SPDX actor `Person: `, which strict consumers reject.
#[must_use]
pub fn extract_author(manifest: &serde_json::Value) -> Option<String> {
    let author = manifest.get("author")?;
    let name = author.as_str().or_else(|| author.get("name")?.as_str())?;
    (!name.trim().is_empty()).then(|| name.to_string())
}

/// Extracts the homepage field from a manifest.
pub fn extract_homepage(manifest: &serde_json::Value) -> Option<String> {
    manifest.get("homepage").and_then(|v| v.as_str()).map(ToString::to_string)
}

/// Extracts the license from either the modern `license` field or the legacy
/// `licenses` field.
pub fn extract_license(manifest: &serde_json::Value) -> Option<String> {
    manifest
        .get("license")
        .and_then(extract_license_field)
        .or_else(|| manifest.get("licenses").and_then(extract_license_field))
}

fn extract_license_field(field: &serde_json::Value) -> Option<String> {
    if let Some(license) = field.as_str() {
        return (!license.is_empty()).then(|| license.to_string());
    }
    if let Some(entries) = field.as_array() {
        let licenses: Vec<&str> = entries.iter().filter_map(extract_license_type).collect();
        return match licenses.as_slice() {
            [] => None,
            [license] => Some((*license).to_string()),
            licenses => Some(format!("({})", licenses.join(" OR "))),
        };
    }
    extract_license_type(field).map(ToString::to_string)
}

fn extract_license_type(entry: &serde_json::Value) -> Option<&str> {
    if let Some(license) = entry.as_str().filter(|license| !license.is_empty()) {
        return Some(license);
    }
    let entry = entry.as_object()?;
    for key in ["type", "name"] {
        if let Some(license) =
            entry.get(key).and_then(serde_json::Value::as_str).filter(|license| !license.is_empty())
        {
            return Some(license);
        }
    }
    None
}

mod runtime;

mod initialization;

mod serialization;
use serialization::{normalize_dependency_fields, serialize_with_indent};
