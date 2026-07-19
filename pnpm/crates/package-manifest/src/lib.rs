use std::{
    fs,
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

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum PackageManifestError {
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    Serialization(serde_json::Error), // TODO: remove derive(From), split this variant

    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR))]
    Io(std::io::Error), // TODO: remove derive(From), split this variant

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

impl PackageManifest {
    fn create_init_package_json(name: &str) -> Value {
        json!({
            "name": name,
            "version": "1.0.0",
            "description": "",
            "main": "index.js",
            "scripts": {
              "test": r#"echo "Error: no test specified" && exit 1"#
            },
            "keywords": [],
            "author": "",
            "license": "ISC"
        })
    }

    fn write_to_file(path: &Path) -> Result<String, PackageManifestError> {
        let manifest = PackageManifest::init_value_for(path);
        let contents = serialize_with_indent(&manifest, DEFAULT_INDENT)?;
        fs::write(path, format!("{contents}\n"))?; // TODO: forbid overwriting existing files
        Ok(contents)
    }

    /// The scaffold manifest `pnpm init` (and [`Self::create_if_needed`])
    /// produces for `path`, named after the containing directory.
    #[must_use]
    pub fn init_value_for(path: &Path) -> Value {
        let name = path
            .parent()
            .and_then(|folder| folder.file_name())
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("");
        PackageManifest::create_init_package_json(name)
    }

    /// Write `contents` to `path` atomically: a sibling temp file is written
    /// and fsynced, then renamed over `path`. A crash or write error therefore
    /// never leaves a truncated or partial `package.json` behind, matching the
    /// `write-file-atomic` guarantee.
    fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut tmp = NamedTempFile::new_in(dir)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.as_file().sync_all()?;
        // A NamedTempFile is created 0o600; preserve the original file's mode
        // when overwriting an existing package.json (write-file-atomic does the
        // same) so the rename doesn't silently tighten its permissions.
        if let Ok(metadata) = fs::metadata(path) {
            tmp.as_file().set_permissions(metadata.permissions())?;
        }
        tmp.persist(path).map_err(|err| err.error)?;
        Ok(())
    }

    fn read_from_file(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        let contents = fs::read_to_string(&path)?;
        let mut value: Value = serde_json::from_str(&contents)?;
        let mut on_disk = value.clone();
        normalize_dependency_fields(&mut on_disk);
        convert_engines_runtime_to_dependencies(&mut value, "devEngines", "devDependencies");
        convert_engines_runtime_to_dependencies(&mut value, "engines", "dependencies");
        Ok(PackageManifest {
            path,
            value,
            insert_final_newline: contents.ends_with('\n'),
            indent: detect_indent(&contents).to_string(),
            on_disk: Some(on_disk),
        })
    }

    pub fn init(path: &Path) -> Result<(), PackageManifestError> {
        if path.exists() {
            return Err(PackageManifestError::AlreadyExist);
        }
        let contents = PackageManifest::write_to_file(path)?;
        println!("Wrote to {path}\n\n{contents}", path = path.display());
        Ok(())
    }

    pub fn from_path(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        if !path.exists() {
            return Err(PackageManifestError::NoImporterManifestFound(path.display().to_string()));
        }

        PackageManifest::read_from_file(path)
    }

    pub fn create_if_needed(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        if !path.exists() {
            PackageManifest::write_to_file(&path)?;
        }
        // Read the scaffold back rather than assembling the manifest by
        // hand, so its formatting and no-op-save baseline are derived from
        // the file the same way as for a pre-existing manifest.
        PackageManifest::read_from_file(path)
    }

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

    /// Persist the manifest in its on-disk shape (`devEngines` folded back,
    /// dependency fields normalized) and return that shape.
    ///
    /// The write preserves the source file's indentation and final-newline
    /// state, and is skipped entirely when the file already encodes the
    /// same manifest — so a no-op save never churns formatting or mtime.
    pub fn save_and_get_written_value(&mut self) -> Result<Value, PackageManifestError> {
        let mut value = self.value.clone();
        convert_dependencies_to_engines_runtime(&mut value, "devDependencies", "devEngines")?;
        convert_dependencies_to_engines_runtime(&mut value, "dependencies", "engines")?;
        normalize_dependency_fields(&mut value);
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
        if let Some(field) = self.value.get_mut(dependency_type) {
            if let Some(dependencies) = field.as_object_mut() {
                dependencies.insert(name.to_string(), Value::String(version.to_string()));
            } else {
                return Err(PackageManifestError::InvalidAttribute(
                    "dependencies attribute should be an object".to_string(),
                ));
            }
        } else {
            let mut dependencies = Map::<String, Value>::new();
            dependencies.insert(name.to_string(), Value::String(version.to_string()));
            self.value[dependency_type] = Value::Object(dependencies);
        }
        const INSTALL_GROUPS: [DependencyGroup; 3] =
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        if INSTALL_GROUPS.contains(&dependency_group) {
            let removed = [name.to_string()];
            for group in INSTALL_GROUPS {
                if group != dependency_group {
                    self.remove_from_object(group.into(), &removed);
                }
            }
        }
        Ok(())
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

/// Runtime aliases recognised by `devEngines.runtime` /
/// `engines.runtime` reification.
const RUNTIME_NAMES: [&str; 3] = ["node", "deno", "bun"];

/// Whether `alias` names a runtime pnpm can download and manage
/// (`node` / `deno` / `bun`).
#[must_use]
pub fn is_runtime_alias(alias: &str) -> bool {
    RUNTIME_NAMES.contains(&alias)
}

/// Reify `devEngines.runtime` / `engines.runtime` entries with
/// `onFail: "download"` into the matching `devDependencies` /
/// `dependencies` slot as `runtime:<version>` specifiers.
///
/// This makes the lockfile entry the resolver writes
/// (`node@runtime:24.6.0`, etc.) visible to the
/// `satisfies_package_manifest` flat-record diff under the manifest's
/// own dependency map. Without this step a manifest that declares its
/// runtime exclusively through `devEngines.runtime` fails the frozen-
/// lockfile staleness check as a spurious "dependency was removed".
///
/// The `WebContainer` "no runtime download" branch is intentionally
/// omitted: pacquet does not run in `WebContainer`.
pub fn convert_engines_runtime_to_dependencies(
    manifest: &mut Value,
    engines_field: &str,
    deps_field: &str,
) {
    // Collect first, mutate after — avoids a simultaneous shared+mutable
    // borrow of the manifest while reading `engines_field` and writing
    // `deps_field`.
    let mut to_insert: Vec<(&'static str, String)> = Vec::new();
    let Some(runtime_entry) =
        manifest.get(engines_field).and_then(|engines| engines.get("runtime"))
    else {
        return;
    };
    for runtime_name in RUNTIME_NAMES {
        if manifest.get(deps_field).and_then(|deps| deps.get(runtime_name)).is_some() {
            continue;
        }
        let runtimes: &[Value] = match runtime_entry {
            Value::Array(arr) => arr.as_slice(),
            single @ Value::Object(_) => std::slice::from_ref(single),
            _ => continue,
        };
        let Some(runtime) = runtimes
            .iter()
            .find(|runtime| runtime.get("name").and_then(Value::as_str) == Some(runtime_name))
        else {
            continue;
        };
        if runtime.get("onFail").and_then(Value::as_str) != Some("download") {
            continue;
        }
        let Some(version) = runtime.get("version").and_then(Value::as_str) else {
            continue;
        };
        to_insert.push((runtime_name, format!("runtime:{}", version.trim())));
    }
    if to_insert.is_empty() {
        return;
    }
    let Some(manifest_obj) = manifest.as_object_mut() else {
        return;
    };
    let deps =
        manifest_obj.entry(deps_field.to_string()).or_insert_with(|| Value::Object(Map::new()));
    let Some(deps_obj) = deps.as_object_mut() else {
        return;
    };
    for (name, spec) in to_insert {
        deps_obj.insert(name.to_string(), Value::String(spec));
    }
}

/// Apply the configured runtime failure policy to both engine fields.
///
/// A non-download policy removes only dependency entries synthesized from a
/// `runtime:` specifier; an explicit ordinary dependency with the same name is
/// preserved. `download` re-runs the normal engine-to-dependency conversion.
pub fn apply_runtime_on_fail_override(manifest: &mut Value, on_fail_override: &str) {
    for (engines_field, deps_field) in
        [("devEngines", "devDependencies"), ("engines", "dependencies")]
    {
        let Some(runtime_entry) =
            manifest.get_mut(engines_field).and_then(|engines| engines.get_mut("runtime"))
        else {
            continue;
        };
        match runtime_entry {
            Value::Array(runtimes) => {
                for runtime in runtimes {
                    if let Some(runtime) = runtime.as_object_mut() {
                        runtime.insert(
                            "onFail".to_string(),
                            Value::String(on_fail_override.to_string()),
                        );
                    }
                }
            }
            Value::Object(runtime) => {
                runtime.insert("onFail".to_string(), Value::String(on_fail_override.to_string()));
            }
            _ => continue,
        }
        if on_fail_override == "download" {
            convert_engines_runtime_to_dependencies(manifest, engines_field, deps_field);
            continue;
        }
        let Some(deps) = manifest.get_mut(deps_field).and_then(Value::as_object_mut) else {
            continue;
        };
        for runtime_name in RUNTIME_NAMES {
            if deps
                .get(runtime_name)
                .and_then(Value::as_str)
                .is_some_and(|specifier| specifier.starts_with("runtime:"))
            {
                deps.remove(runtime_name);
            }
        }
    }
}

/// Return the minimum Node.js version declared by `devEngines.runtime` or
/// `engines.runtime`, in that precedence order.
#[must_use]
pub fn node_version_from_engines_runtime(manifest: &Value) -> Option<String> {
    for engines_field in ["devEngines", "engines"] {
        let Some(runtime_entry) =
            manifest.get(engines_field).and_then(|value| value.get("runtime"))
        else {
            continue;
        };
        let runtimes = match runtime_entry {
            Value::Array(runtimes) => runtimes.as_slice(),
            runtime @ Value::Object(_) => std::slice::from_ref(runtime),
            _ => continue,
        };
        let Some(version) = runtimes.iter().find_map(|runtime| {
            (runtime.get("name").and_then(Value::as_str) == Some("node"))
                .then(|| runtime.get("version").and_then(Value::as_str))
                .flatten()
        }) else {
            continue;
        };
        if let Ok(range) = Range::parse(version)
            && let Some(version) = range.min_version()
        {
            return Some(version.to_string());
        }
    }
    None
}

/// pnpm's on-write manifest normalization: within each dependency field,
/// sort the entries by name, and drop the field entirely when it holds no
/// entries.
fn normalize_dependency_fields(manifest: &mut Value) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        let is_empty_object = match manifest.get_mut(field) {
            Some(Value::Object(deps)) => {
                deps.sort_keys();
                deps.is_empty()
            }
            _ => continue,
        };
        if is_empty_object {
            manifest.remove(field);
        }
    }
}

/// The indentation unit of a JSON document: the leading whitespace of its
/// first indented line. Empty for a single-line (or unindented) document,
/// which then round-trips back to its compact form.
fn detect_indent(contents: &str) -> &str {
    contents
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            (!trimmed.is_empty() && trimmed.len() < line.len())
                .then(|| &line[..line.len() - trimmed.len()])
        })
        .unwrap_or("")
}

/// Serialize with the manifest's own indentation unit; an empty unit
/// produces a compact single-line document. At most the first 10
/// characters of the unit are used — the cap `JSON.stringify` applies to
/// its `space` argument, which pnpm writes manifests through — so a
/// pathologically indented source file can't amplify the output.
fn serialize_with_indent(value: &Value, indent: &str) -> Result<String, serde_json::Error> {
    if indent.is_empty() {
        return serde_json::to_string(value);
    }
    let indent = match indent.char_indices().nth(10) {
        Some((cap, _)) => &indent[..cap],
        None => indent,
    };
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value.serialize(&mut serializer)?;
    Ok(String::from_utf8(out).expect("serde_json emits UTF-8"))
}

/// Fold `runtime:<version>` dependency entries back into
/// `devEngines.runtime` / `engines.runtime` before writing a manifest.
///
/// The in-memory dependency form drives resolution and lockfile checks,
/// while the on-disk manifest keeps the `devEngines.runtime` /
/// `engines.runtime` contract.
///
/// Mutates `manifest` in place and removes consumed `runtime:` dependency
/// entries. Returns `InvalidAttribute` when a field shape prevents a
/// lossless write.
pub fn convert_dependencies_to_engines_runtime(
    manifest: &mut Value,
    deps_field: &str,
    engines_field: &str,
) -> Result<(), PackageManifestError> {
    if manifest.get(deps_field).is_some_and(|deps| !deps.is_object()) {
        return Err(PackageManifestError::InvalidAttribute(format!(
            "the {deps_field} field must be an object",
        )));
    }
    for runtime_name in RUNTIME_NAMES {
        let version = manifest
            .get(deps_field)
            .and_then(Value::as_object)
            .and_then(|deps| deps.get(runtime_name))
            .and_then(Value::as_str)
            .and_then(|dep| dep.strip_prefix("runtime:"))
            .map(str::trim)
            .map(str::to_string);
        if let Some(version) = version {
            upsert_runtime_entry(manifest, engines_field, runtime_name, &version)?;
            if let Some(deps) = manifest.get_mut(deps_field).and_then(Value::as_object_mut) {
                deps.remove(runtime_name);
            }
        } else {
            remove_managed_runtime_entry(manifest, engines_field, runtime_name);
        }
    }
    Ok(())
}

fn remove_managed_runtime_entry(manifest: &mut Value, engines_field: &str, runtime_name: &str) {
    let Some(engines) = manifest.get_mut(engines_field).and_then(Value::as_object_mut) else {
        return;
    };
    let remove_runtime = match engines.get_mut("runtime") {
        Some(Value::Array(runtimes)) => {
            runtimes.retain(|runtime| !is_managed_runtime_entry(runtime, runtime_name));
            runtimes.is_empty()
        }
        Some(runtime) if is_managed_runtime_entry(runtime, runtime_name) => true,
        _ => false,
    };
    if remove_runtime {
        engines.remove("runtime");
    }
}

fn is_managed_runtime_entry(runtime: &Value, runtime_name: &str) -> bool {
    runtime.get("name").and_then(Value::as_str) == Some(runtime_name)
        && runtime.get("onFail").and_then(Value::as_str) == Some("download")
        && runtime.get("version").and_then(Value::as_str).is_some()
}

fn upsert_runtime_entry(
    manifest: &mut Value,
    engines_field: &str,
    runtime_name: &str,
    version: &str,
) -> Result<(), PackageManifestError> {
    let runtime_entry = json!({
        "name": runtime_name,
        "version": version,
        "onFail": "download",
    });
    let engines = ensure_object_field(manifest, engines_field)?;
    match engines.get_mut("runtime") {
        None | Some(Value::Null) => {
            engines.insert("runtime".to_string(), runtime_entry);
        }
        Some(Value::Array(runtimes)) => {
            if let Some(existing) = runtimes
                .iter_mut()
                .find(|runtime| runtime.get("name").and_then(Value::as_str) == Some(runtime_name))
            {
                merge_runtime_entry(existing, runtime_name, version)?;
            } else {
                runtimes.push(runtime_entry);
            }
        }
        Some(Value::Object(runtime))
            if runtime.get("name").and_then(Value::as_str) == Some(runtime_name) =>
        {
            runtime.insert("name".to_string(), Value::String(runtime_name.to_string()));
            runtime.insert("version".to_string(), Value::String(version.to_string()));
            runtime.insert("onFail".to_string(), Value::String("download".to_string()));
        }
        Some(existing) => {
            *existing = Value::Array(vec![existing.clone(), runtime_entry]);
        }
    }
    Ok(())
}

fn ensure_object_field<'a>(
    manifest: &'a mut Value,
    field: &str,
) -> Result<&'a mut Map<String, Value>, PackageManifestError> {
    let Some(root) = manifest.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(
            "the manifest root must be an object".to_string(),
        ));
    };
    let value = root.entry(field.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if value.is_null() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().ok_or_else(|| {
        PackageManifestError::InvalidAttribute(format!("the {field} field must be an object"))
    })
}

fn merge_runtime_entry(
    runtime: &mut Value,
    runtime_name: &str,
    version: &str,
) -> Result<(), PackageManifestError> {
    let Some(runtime) = runtime.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(
            "runtime entries must be objects".to_string(),
        ));
    };
    runtime.insert("name".to_string(), Value::String(runtime_name.to_string()));
    runtime.insert("version".to_string(), Value::String(version.to_string()));
    runtime.insert("onFail".to_string(), Value::String("download".to_string()));
    Ok(())
}

/// Read `<dir>/package.json` if it exists, returning `Ok(None)` when the file
/// is absent. Other IO errors and JSON parse errors propagate.
///
/// A missing file is the only case that maps to `Ok(None)`; malformed JSON
/// surfaces as a `BAD_PACKAGE_JSON` error and other IO errors propagate.
pub fn safe_read_package_json_from_dir(dir: &Path) -> Result<Option<Value>, PackageManifestError> {
    let path = dir.join("package.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(PackageManifestError::Io(err)),
    };
    serde_json::from_str(&text).map(Some).map_err(PackageManifestError::Serialization)
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
#[must_use]
pub fn manifest_requires_build(manifest: &Value) -> bool {
    manifest.get("scripts").and_then(Value::as_object).is_some_and(|scripts| {
        scripts.contains_key("preinstall")
            || scripts.contains_key("install")
            || scripts.contains_key("postinstall")
    })
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
pub fn extract_author(manifest: &serde_json::Value) -> Option<String> {
    let author = manifest.get("author")?;
    if let Some(s) = author.as_str() {
        return Some(s.to_string());
    }
    author.get("name").and_then(|n| n.as_str()).map(ToString::to_string)
}

/// Extracts the homepage field from a manifest.
pub fn extract_homepage(manifest: &serde_json::Value) -> Option<String> {
    manifest.get("homepage").and_then(|v| v.as_str()).map(ToString::to_string)
}
