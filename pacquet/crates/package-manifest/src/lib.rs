use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use derive_more::{Display, Error, From};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use strum::IntoStaticStr;

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum PackageManifestError {
    #[diagnostic(code(pacquet_package_manifest::serialization_error))]
    Serialization(serde_json::Error), // TODO: remove derive(From), split this variant

    #[diagnostic(code(pacquet_package_manifest::io_error))]
    Io(std::io::Error), // TODO: remove derive(From), split this variant

    #[display("package.json file already exists")]
    #[diagnostic(
        code(pacquet_package_manifest::already_exist_error),
        help("Your current working directory already has a package.json file.")
    )]
    AlreadyExist,

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("invalid attribute: {_0}")]
    #[diagnostic(code(pacquet_package_manifest::invalid_attribute))]
    InvalidAttribute(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("No package.json was found in {_0}")]
    #[diagnostic(code(pacquet_package_manifest::no_import_manifest_found))]
    NoImporterManifestFound(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Missing script: {_0:?}")]
    #[diagnostic(code(pacquet_package_manifest::no_script_error))]
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

/// Content of the `package.json` files and its path.
#[derive(Clone)]
pub struct PackageManifest {
    path: PathBuf,
    value: Value, // TODO: convert this into a proper struct + an array of keys order
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

    fn write_to_file(path: &Path) -> Result<(Value, String), PackageManifestError> {
        let name = path
            .parent()
            .and_then(|folder| folder.file_name())
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("");
        let manifest = PackageManifest::create_init_package_json(name);
        let contents = serde_json::to_string_pretty(&manifest)?;
        fs::write(path, &contents)?; // TODO: forbid overwriting existing files
        Ok((manifest, contents))
    }

    fn read_from_file(path: &Path) -> Result<Value, PackageManifestError> {
        let contents = fs::read_to_string(path)?;
        let mut value: Value = serde_json::from_str(&contents)?;
        // Mirror upstream pnpm's `convertManifestAfterRead` at
        // <https://github.com/pnpm/pnpm/blob/9cad8274fd/workspace/project-manifest-reader/src/index.ts#L227-L231>:
        // declarations under `devEngines.runtime` / `engines.runtime`
        // with `onFail: "download"` are reified into the matching
        // dependencies bucket so the lockfile entry the resolver writes
        // (e.g. `node@runtime:24.6.0`) lines up with the manifest's
        // flat-record view during the frozen-lockfile staleness check.
        convert_engines_runtime_to_dependencies(&mut value, "devEngines", "devDependencies");
        convert_engines_runtime_to_dependencies(&mut value, "engines", "dependencies");
        Ok(value)
    }

    pub fn init(path: &Path) -> Result<(), PackageManifestError> {
        if path.exists() {
            return Err(PackageManifestError::AlreadyExist);
        }
        let (_, contents) = PackageManifest::write_to_file(path)?;
        println!("Wrote to {path}\n\n{contents}", path = path.display());
        Ok(())
    }

    pub fn from_path(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        if !path.exists() {
            return Err(PackageManifestError::NoImporterManifestFound(path.display().to_string()));
        }

        let value = PackageManifest::read_from_file(&path)?;
        Ok(PackageManifest { path, value })
    }

    pub fn create_if_needed(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        let value = if path.exists() {
            PackageManifest::read_from_file(&path)?
        } else {
            PackageManifest::write_to_file(&path).map(|(value, _)| value)?
        };

        Ok(PackageManifest { path, value })
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
    /// dependency maps before downstream consumers see it (mirrors
    /// upstream's `readPackageHook` returning a modified manifest).
    /// Mutations stay in memory — there is no implicit `save`, so the
    /// user's on-disk `package.json` is untouched.
    pub fn value_mut(&mut self) -> &'_ mut Value {
        &mut self.value
    }

    pub fn save(&self) -> Result<(), PackageManifestError> {
        let mut file = fs::File::create(&self.path)?;
        let contents = serde_json::to_string_pretty(&self.value)?;
        file.write_all(contents.as_bytes())?;
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
    ///
    /// Mirrors pnpm's `parseBareSpecifier`. Reference:
    /// <https://github.com/pnpm/pnpm/blob/1819226b51/resolving/npm-resolver/src/parseBareSpecifier.ts>
    #[must_use]
    pub fn resolve_registry_dependency<'a>(
        key: &'a str,
        bare_specifier: &'a str,
    ) -> (&'a str, &'a str) {
        let Some(rest) = bare_specifier.strip_prefix("npm:") else {
            return (key, bare_specifier);
        };
        // pnpm's parseBareSpecifier uses `lastIndexOf('@')` and treats
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
        Ok(())
    }

    /// Names eligible for `pnpm remove` to target.
    ///
    /// When `save_type` is `Some`, the keys of just that field; when
    /// `None`, the union of `dependencies`, `devDependencies`, and
    /// `optionalDependencies` (peer dependencies excluded), preserving
    /// first-seen order. Mirrors pnpm's `getAllDependenciesFromManifest`
    /// (called without `autoInstallPeers`) at
    /// <https://github.com/pnpm/pnpm/blob/9cad8274fd/pkg-manifest/utils/src/getAllDependenciesFromManifest.ts>,
    /// the set `pnpm remove` validates removal targets against.
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
    /// Ports pnpm's
    /// [`removeDeps`](https://github.com/pnpm/pnpm/blob/9cad8274fd/installing/deps-installer/src/uninstall/removeDeps.ts):
    /// when `save_type` is `Some`, only that field is touched; otherwise
    /// every field in pnpm's `DEPENDENCIES_FIELDS` (`optionalDependencies`,
    /// `dependencies`, `devDependencies`) is scanned. `peerDependencies`
    /// and `dependenciesMeta` entries for the removed names are always
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

/// Runtime aliases recognised by pnpm's `devEngines.runtime` /
/// `engines.runtime` reification. Matches upstream's
/// [`RUNTIME_NAMES`](https://github.com/pnpm/pnpm/blob/9cad8274fd/pkg-manifest/utils/src/convertEnginesRuntimeToDependencies.ts#L8).
const RUNTIME_NAMES: [&str; 3] = ["node", "deno", "bun"];

/// Reify `devEngines.runtime` / `engines.runtime` entries with
/// `onFail: "download"` into the matching `devDependencies` /
/// `dependencies` slot as `runtime:<version>` specifiers.
///
/// Ports upstream's
/// [`convertEnginesRuntimeToDependencies`](https://github.com/pnpm/pnpm/blob/9cad8274fd/pkg-manifest/utils/src/convertEnginesRuntimeToDependencies.ts#L10-L45)
/// so the lockfile entry the resolver writes
/// (`node@runtime:24.6.0`, etc.) is visible to the
/// `satisfies_package_manifest` flat-record diff under the manifest's
/// own dependency map. Without this step a manifest that declares its
/// runtime exclusively through `devEngines.runtime` fails the frozen-
/// lockfile staleness check as a spurious "dependency was removed".
///
/// Runtime entries are skipped when:
/// - the target dependency slot already has an explicit entry (the
///   user-declared specifier wins),
/// - the runtime is declared with `onFail` other than `"download"` (the
///   download is opt-in upstream), or
/// - no `version` is set (upstream warns and skips; pacquet skips
///   silently — the staleness check still surfaces the gap downstream).
///
/// `WebContainer`'s "no runtime download" branch upstream is intentionally
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
        to_insert.push((runtime_name, format!("runtime:{version}")));
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

/// Read `<dir>/package.json` if it exists, returning `Ok(None)` when the file
/// is absent. Other IO errors and JSON parse errors propagate.
///
/// Mirrors upstream `safeReadPackageJsonFromDir` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manifest/reader/src/index.ts#L48>.
/// Upstream returns `null` only on `ENOENT`; malformed JSON surfaces as a
/// `BAD_PACKAGE_JSON` error and other IO errors propagate.
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
/// Mirrors upstream `pkgRequiresBuild` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/building/pkg-requires-build/src/index.ts>:
/// true when the package's manifest declares any of `preinstall`, `install`,
/// or `postinstall`, or when the package contains `binding.gyp` or a `.hooks/`
/// directory. Missing manifests, IO errors, and parse errors all collapse to
/// `false` — pacquet cannot meaningfully build a package whose extracted
/// content cannot be inspected.
pub fn pkg_requires_build(pkg_root: &Path) -> bool {
    if pkg_root.join("binding.gyp").exists() || pkg_root.join(".hooks").is_dir() {
        return true;
    }
    let Ok(Some(manifest)) = safe_read_package_json_from_dir(pkg_root) else { return false };
    manifest.get("scripts").and_then(Value::as_object).is_some_and(|scripts| {
        scripts.contains_key("preinstall")
            || scripts.contains_key("install")
            || scripts.contains_key("postinstall")
    })
}

#[cfg(test)]
mod tests;
