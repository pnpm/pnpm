//! Port of upstream's
//! [`createExportableManifest`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/index.ts#L45-L98)
//! — turns a project's on-disk manifest into the manifest that ships
//! inside a published tarball.
//!
//! The pipeline:
//!
//! 1. **Obfuscation.** Strip pnpm-internal fields. With
//!    `skip_manifest_obfuscation` only `pnpm` is dropped; otherwise
//!    `scripts`, `packageManager`, and `pnpm` are dropped and the
//!    surviving `scripts` map loses its publish-lifecycle entries.
//! 2. **Dependency rewriting.** Each `dependencies` /
//!    `devDependencies` / `optionalDependencies` / `peerDependencies`
//!    value runs through the workspace → catalog → jsr replacers in
//!    sequence, turning `workspace:` / `catalog:` / `jsr:` specifiers
//!    into the concrete specifiers the registry understands.
//! 3. **`publishConfig` override.** Whitelisted `publishConfig` keys
//!    are hoisted onto the manifest root.
//! 4. **README embedding** (opt-in).
//! 5. **`transform`.** Required-field validation plus `bin` /
//!    `peerDependenciesMeta` / `repository` normalization.
//!
//! Upstream composes the dependency replacers with a `combineConverters`
//! higher-order function; the Rust port calls them in a straight
//! sequence ([`convert_dependency_for_publish`]) rather than threading
//! a list of closures.
//!
//! `beforePacking` pnpmfile hooks are not applied here: pacquet's
//! pnpmfile bridge (`pacquet_hooks::PnpmfileHooks`) does not yet
//! expose that hook, so there is no source to feed it. The step lands
//! when the bridge grows a `beforePacking` entry point.

use crate::{
    replace::{
        ReplaceWorkspaceProtocolError, replace_workspace_protocol,
        replace_workspace_protocol_peer_dependency,
    },
    transform::{TransformError, transform},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_resolver::{
    CatalogResolutionError, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_resolving_jsr_specifier_parser::{ParseJsrSpecifierError, parse_jsr_specifier};
use serde_json::{Map, Value};
use std::{fs, io, path::Path};

/// Lifecycle scripts removed from the published manifest's `scripts`
/// map during obfuscation, so they don't re-run when the package is
/// installed from the registry. Mirrors upstream's `PREPUBLISH_SCRIPTS`
/// at [`index.ts:19-26`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/index.ts#L19-L26).
const PREPUBLISH_SCRIPTS: &[&str] =
    &["prepublishOnly", "prepack", "prepare", "postpack", "publish", "postpublish"];

/// Manifest keys hoisted from `publishConfig` onto the manifest root.
/// Mirrors upstream's `PUBLISH_CONFIG_WHITELIST` at
/// [`overridePublishConfig.ts:5-31`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/overridePublishConfig.ts#L5-L31).
const PUBLISH_CONFIG_WHITELIST: &[&str] = &[
    "bin",
    "engines",
    "type",
    "imports",
    "main",
    "module",
    "typings",
    "types",
    "exports",
    "browser",
    "esnext",
    "es2015",
    "unpkg",
    "umd:main",
    "os",
    "cpu",
    "libc",
    "typesVersions",
];

/// Options for [`create_exportable_manifest`], mirroring the subset of
/// upstream's `MakePublishManifestOptions` pacquet currently supports.
pub struct CreateExportableManifestOptions<'a> {
    /// Parsed workspace catalogs, used to resolve `catalog:` specifiers.
    pub catalogs: &'a Catalogs,
    /// Where workspace dependencies are installed. Defaults to
    /// `<dir>/node_modules` when `None`.
    pub modules_dir: Option<&'a Path>,
    /// Keep `packageManager` and publish-lifecycle scripts in the
    /// packed manifest; only the `pnpm` field is stripped.
    pub skip_manifest_obfuscation: bool,
    /// Embed the project's `README.md` into the manifest's `readme`
    /// field when one is present and the manifest doesn't already
    /// declare `readme`.
    pub embed_readme: bool,
}

/// Failures from [`create_exportable_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum CreateExportableManifestError {
    #[diagnostic(transparent)]
    ReplaceWorkspaceProtocol(#[error(source)] ReplaceWorkspaceProtocolError),

    #[diagnostic(transparent)]
    Catalog(#[error(source)] CatalogResolutionError),

    #[diagnostic(transparent)]
    Jsr(#[error(source)] ParseJsrSpecifierError),

    #[diagnostic(transparent)]
    Transform(#[error(source)] TransformError),

    #[display("Failed to read README from {dir}: {source}")]
    #[diagnostic(code(pacquet_exportable_manifest::read_readme))]
    ReadReadme {
        dir: String,
        #[error(source)]
        source: io::Error,
    },
}

/// Build the publish manifest for the project at `dir` from its
/// `original_manifest`.
pub fn create_exportable_manifest(
    dir: &Path,
    original_manifest: &Value,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<Value, CreateExportableManifestError> {
    let empty = Map::new();
    let original = original_manifest.as_object().unwrap_or(&empty);

    let mut publish = if opts.skip_manifest_obfuscation {
        omit_keys(original, &["pnpm"])
    } else {
        let mut publish = omit_keys(original, &["scripts", "packageManager", "pnpm"]);
        if let Some(scripts) = original.get("scripts").and_then(Value::as_object) {
            publish.insert(
                "scripts".to_string(),
                Value::Object(omit_keys(scripts, PREPUBLISH_SCRIPTS)),
            );
        }
        publish
    };

    for field in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(deps) =
            make_publish_dependencies(dir, original.get(field), opts, DependencyKind::Regular)?
        {
            publish.insert(field.to_string(), deps);
        }
    }
    if let Some(peer) = original.get("peerDependencies") {
        let converted = make_publish_dependencies(dir, Some(peer), opts, DependencyKind::Peer)?
            .unwrap_or_else(|| Value::Object(Map::new()));
        publish.insert("peerDependencies".to_string(), converted);
    }

    override_publish_config(&mut publish);

    if opts.embed_readme
        && !publish.contains_key("readme")
        && let Some(readme) = read_readme_file(dir).map_err(|source| {
            CreateExportableManifestError::ReadReadme { dir: dir.display().to_string(), source }
        })?
    {
        publish.insert("readme".to_string(), Value::String(readme));
    }

    transform(&mut publish).map_err(CreateExportableManifestError::Transform)?;
    Ok(Value::Object(publish))
}

/// Whether a dependency map's specifiers carry the regular-dependency
/// or the peer-dependency workspace-protocol semantics. Peer specs
/// allow the broader comparator set (`>=`, `<=`, ...).
#[derive(Clone, Copy)]
enum DependencyKind {
    Regular,
    Peer,
}

/// Rewrite every specifier in one dependency map for publishing.
/// Returns `None` when the field is absent or not an object, so the
/// caller leaves the manifest's existing value in place.
fn make_publish_dependencies(
    dir: &Path,
    dependencies: Option<&Value>,
    opts: &CreateExportableManifestOptions<'_>,
    kind: DependencyKind,
) -> Result<Option<Value>, CreateExportableManifestError> {
    let Some(dependencies) = dependencies.and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut out = Map::with_capacity(dependencies.len());
    for (name, spec) in dependencies {
        let Some(spec) = spec.as_str() else {
            // A non-string specifier can't be a workspace / catalog /
            // jsr protocol; carry it through unchanged.
            out.insert(name.clone(), spec.clone());
            continue;
        };
        let converted = convert_dependency_for_publish(name, spec, dir, opts, kind)?;
        out.insert(name.clone(), Value::String(converted));
    }
    Ok(Some(Value::Object(out)))
}

/// Run one specifier through the workspace → catalog → jsr replacers in
/// sequence, returning the registry-ready specifier.
fn convert_dependency_for_publish(
    dep_name: &str,
    spec: &str,
    dir: &Path,
    opts: &CreateExportableManifestOptions<'_>,
    kind: DependencyKind,
) -> Result<String, CreateExportableManifestError> {
    let after_workspace = match kind {
        DependencyKind::Regular => {
            replace_workspace_protocol(dep_name, spec, dir, opts.modules_dir)
        }
        DependencyKind::Peer => {
            replace_workspace_protocol_peer_dependency(dep_name, spec, dir, opts.modules_dir)
        }
    }
    .map_err(CreateExportableManifestError::ReplaceWorkspaceProtocol)?;
    let after_catalog = replace_catalog_protocol(dep_name, &after_workspace, opts.catalogs)?;
    replace_jsr_protocol(dep_name, &after_catalog)
}

/// Dereference a `catalog:` specifier; pass any other specifier
/// through unchanged.
fn replace_catalog_protocol(
    alias: &str,
    spec: &str,
    catalogs: &Catalogs,
) -> Result<String, CreateExportableManifestError> {
    let wanted = WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok(found.resolution.specifier),
        CatalogResolutionResult::Unused => Ok(spec.to_string()),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => {
            Err(CreateExportableManifestError::Catalog(misconfiguration.error))
        }
    }
}

/// Rewrite a `jsr:` specifier into its `npm:`-aliased form; pass any
/// other specifier through unchanged.
fn replace_jsr_protocol(
    dep_name: &str,
    spec: &str,
) -> Result<String, CreateExportableManifestError> {
    match parse_jsr_specifier(spec, Some(dep_name)).map_err(CreateExportableManifestError::Jsr)? {
        Some(jsr) => {
            Ok(create_npm_aliased_specifier(&jsr.npm_pkg_name, jsr.version_selector.as_deref()))
        }
        None => Ok(spec.to_string()),
    }
}

/// Build an `npm:<name>[@<selector>]` specifier. Mirrors upstream's
/// `createNpmAliasedSpecifier` at
/// [`index.ts:223-229`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/index.ts#L223-L229).
fn create_npm_aliased_specifier(npm_pkg_name: &str, version_selector: Option<&str>) -> String {
    match version_selector {
        Some(selector) if !selector.is_empty() => format!("npm:{npm_pkg_name}@{selector}"),
        _ => format!("npm:{npm_pkg_name}"),
    }
}

/// Hoist whitelisted `publishConfig` keys onto the manifest root,
/// dropping them from `publishConfig` (and removing `publishConfig`
/// entirely once empty). Mirrors upstream's `overridePublishConfig`.
fn override_publish_config(publish: &mut Map<String, Value>) {
    let Some(publish_config) = publish.get("publishConfig").and_then(Value::as_object).cloned()
    else {
        return;
    };
    let mut remaining = Map::new();
    let mut hoisted = Vec::new();
    for (key, value) in publish_config {
        if PUBLISH_CONFIG_WHITELIST.contains(&key.as_str()) {
            hoisted.push((key, value));
        } else {
            remaining.insert(key, value);
        }
    }
    // Hoisting after the partition keeps an existing root key in its
    // original slot (`insert` updates in place) while appending a
    // genuinely new key, matching the JS assignment's object semantics.
    for (key, value) in hoisted {
        publish.insert(key, value);
    }
    if remaining.is_empty() {
        publish.shift_remove("publishConfig");
    } else {
        publish.insert("publishConfig".to_string(), Value::Object(remaining));
    }
}

/// Read a root `README.md` (case-insensitive) for embedding. Only a
/// regular file is embedded — a symlink is skipped so it can't leak
/// the contents of a target outside the project. Mirrors upstream's
/// `readReadmeFile` at
/// [`index.ts:36-43`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/index.ts#L36-L43).
fn read_readme_file(dir: &Path) -> io::Result<Option<String>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case("readme.md") {
            return fs::read_to_string(entry.path()).map(Some);
        }
    }
    Ok(None)
}

/// Clone `map` without the entries named in `keys`, preserving the
/// order of the surviving entries. Replaces upstream's `ramda.omit`.
fn omit_keys(map: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    map.iter()
        .filter(|(key, _)| !keys.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
