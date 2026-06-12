//! Apply `packageExtensions` to dependency manifests at resolve time.
//!
//! Pacquet port of upstream's
//! [`createPackageExtender`](https://github.com/pnpm/pnpm/blob/39101f5e37/hooks/read-package-hook/src/createPackageExtender.ts).
//! Upstream wires this hook into the resolver's `readPackageHook`
//! chain; pacquet calls it from the deps-resolver's per-resolve seam
//! before the resolved `ResolveResult` lands in the wanted-dep cache
//! and the dependency tree walker.
//!
//! Each extension is keyed by a `name[@range]` selector. The hook
//! groups extensions by package name once at construction time, then
//! per-manifest:
//!
//! 1. Looks up the manifest's `name` in the grouped index.
//! 2. For each matched entry, checks the range against the manifest's
//!    `version` with `node_semver` (mirrors upstream's
//!    `semver.satisfies`).
//! 3. Merges each declared field (`dependencies`,
//!    `optionalDependencies`, `peerDependencies`,
//!    `peerDependenciesMeta`) onto the manifest with the extension
//!    *underneath* the manifest's own entries — `{ ...extension,
//!    ...manifest }` — so an extension can only add missing entries,
//!    never overwrite something the package itself declares.
//!
//! The manifest is mutated in place via [`serde_json::Map`] writes.
//! Callers that need to preserve the original manifest must clone it
//! before passing it in (see [`PackageExtender::apply_to_arc`] for the
//! shared-Arc case).

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_config::PackageExtension;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::{Map, Value};
use std::{collections::HashMap, sync::Arc};

/// Returned by [`PackageExtender::new`] when a selector's
/// `@<range>` half fails to parse as a `node-semver` range. Mirrors
/// upstream's
/// [`createPackageExtender`](https://github.com/pnpm/pnpm/blob/39101f5e37/hooks/read-package-hook/src/createPackageExtender.ts#L34-L45)
/// behavior — pnpm passes the raw `bareSpecifier` straight into
/// `semver.satisfies`, which throws a `TypeError` on a malformed
/// range and propagates the failure out of the read-package hook.
/// Pacquet matches that "loud failure" contract but surfaces it
/// earlier (at install start, before any resolution work) so the
/// user sees the bad selector before any tarballs are fetched.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Invalid version range in packageExtensions selector {selector:?}: {range:?} is not a valid semver range"
)]
#[diagnostic(code(INVALID_PACKAGE_EXTENSION_SELECTOR))]
pub struct InvalidPackageExtensionSelector {
    #[error(not(source))]
    pub selector: String,
    pub range: String,
}

/// Pre-grouped `packageExtensions`. Construction parses each selector
/// into its `(name, range)` halves once; per-manifest application is
/// then an O(matches) walk over the entries grouped under the
/// manifest's `name`.
#[derive(Debug, Default, Clone)]
pub struct PackageExtender {
    by_pkg_name: HashMap<String, Vec<ExtensionMatch>>,
}

#[derive(Debug, Clone)]
struct ExtensionMatch {
    /// Pre-parsed semver range from the selector's `@<range>` half,
    /// or [`RangeFilter::Any`] when the selector is bare
    /// (`"is-positive"` — applies to every version). Unparsable
    /// ranges never reach this struct; [`PackageExtender::new`]
    /// surfaces them as [`InvalidPackageExtensionSelector`] before
    /// the install can start. That matches upstream's
    /// `semver.satisfies(version, range)` behavior — a malformed
    /// range throws there too; pacquet just lifts the throw site
    /// from per-manifest application to install-start validation.
    range: RangeFilter,
    extension: PackageExtension,
}

#[derive(Debug, Clone)]
enum RangeFilter {
    Any,
    Range(Range),
}

impl PackageExtender {
    /// Build the extender from the parsed `packageExtensions` map.
    /// Returns an empty extender (which `apply` no-ops on) when the
    /// input is empty.
    pub fn new(
        extensions: &IndexMap<String, PackageExtension>,
    ) -> Result<Self, InvalidPackageExtensionSelector> {
        let mut by_pkg_name: HashMap<String, Vec<ExtensionMatch>> = HashMap::new();
        for (selector, extension) in extensions {
            let parsed = parse_wanted_dependency(selector);
            let Some(alias) = parsed.alias else { continue };
            let range = match parsed.bare_specifier.as_deref() {
                None => RangeFilter::Any,
                Some(range_str) => match range_str.parse() {
                    Ok(range) => RangeFilter::Range(range),
                    Err(_) => {
                        return Err(InvalidPackageExtensionSelector {
                            selector: selector.clone(),
                            range: range_str.to_string(),
                        });
                    }
                },
            };
            by_pkg_name
                .entry(alias)
                .or_default()
                .push(ExtensionMatch { range, extension: extension.clone() });
        }
        Ok(PackageExtender { by_pkg_name })
    }

    /// `true` when no extension entry matches any selector — callers
    /// can skip the per-resolve dispatch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_pkg_name.is_empty()
    }

    /// Apply extensions in place to a single manifest. No-op when the
    /// manifest's `name` doesn't match any selector, when `name` /
    /// `version` are absent, or when no entry's range covers the
    /// manifest's version. Mirrors upstream's
    /// [`extendPkg`](https://github.com/pnpm/pnpm/blob/39101f5e37/hooks/read-package-hook/src/createPackageExtender.ts#L34-L45)
    /// merge order: extension fields land under the manifest's own,
    /// so the manifest's declared entries always win on conflict.
    pub fn apply(&self, manifest: &mut Value) {
        let Some(map) = manifest.as_object_mut() else { return };
        let Some(name) = map.get("name").and_then(Value::as_str).map(str::to_string) else {
            return;
        };
        let Some(entries) = self.by_pkg_name.get(&name) else { return };
        let version =
            map.get("version").and_then(Value::as_str).and_then(|raw| raw.parse::<Version>().ok());
        for entry in entries {
            if !entry_matches(&entry.range, version.as_ref()) {
                continue;
            }
            merge_extension(map, &entry.extension);
        }
    }

    /// Apply extensions to a shared manifest, returning a new `Arc`
    /// when any extension matched and the manifest is therefore
    /// modified. Returns the original `Arc` untouched when no
    /// extension applied — callers can keep using the shared
    /// resolver-cache copy. The clone-on-match shape mirrors how the
    /// upstream `readPackageHook` conceptually returns a fresh
    /// manifest from each consumer's perspective without paying
    /// the deep-clone tax when nothing changed.
    ///
    /// The name-bucket check on its own is not enough — a selector
    /// `is-positive@^1` against a `2.0.0` manifest shares the bucket
    /// but never matches, so a per-entry range pre-check happens
    /// before the deep-clone. The result is the only allocation site
    /// on the resolve hot path; an over-eager clone here would scale
    /// with package count instead of with the actual extension hit
    /// count.
    pub fn apply_to_arc(&self, manifest: Arc<Value>) -> Arc<Value> {
        if self.is_empty() {
            return manifest;
        }
        let Some(map) = manifest.as_object() else { return manifest };
        let Some(name) = map.get("name").and_then(Value::as_str) else { return manifest };
        let Some(entries) = self.by_pkg_name.get(name) else { return manifest };
        let version =
            map.get("version").and_then(Value::as_str).and_then(|raw| raw.parse::<Version>().ok());
        let has_match = entries.iter().any(|entry| entry_matches(&entry.range, version.as_ref()));
        if !has_match {
            return manifest;
        }
        let mut cloned: Value = Value::clone(&manifest);
        self.apply(&mut cloned);
        Arc::new(cloned)
    }

    /// Wrap `self` in a [`pacquet_resolving_deps_resolver::ManifestHook`]
    /// the deps-resolver can plumb through `TreeCtx`. Returns `None`
    /// when the extender is empty so the install pipeline can pass
    /// `None` and skip the per-resolve dispatch entirely. The captured
    /// `Arc<PackageExtender>` keeps the grouped-by-name index alive
    /// across every concurrent resolve.
    #[must_use]
    pub fn into_manifest_hook(self) -> Option<pacquet_resolving_deps_resolver::ManifestHook> {
        if self.is_empty() {
            return None;
        }
        let shared = Arc::new(self);
        Some(Arc::new(move |manifest: Arc<Value>| shared.apply_to_arc(manifest)))
    }
}

/// `true` when an entry's range covers `version`. `RangeFilter::Any`
/// always matches (a bare selector applies to every version); the
/// unparsable case is unreachable here — [`PackageExtender::new`]
/// already returned an error for it before the install began.
fn entry_matches(filter: &RangeFilter, version: Option<&Version>) -> bool {
    match filter {
        RangeFilter::Any => true,
        RangeFilter::Range(range) => version.is_some_and(|version| range.satisfies(version)),
    }
}

fn merge_extension(manifest: &mut Map<String, Value>, extension: &PackageExtension) {
    if let Some(deps) = extension.dependencies.as_ref() {
        merge_string_map(manifest, "dependencies", deps);
    }
    if let Some(deps) = extension.optional_dependencies.as_ref() {
        merge_string_map(manifest, "optionalDependencies", deps);
    }
    if let Some(deps) = extension.peer_dependencies.as_ref() {
        merge_string_map(manifest, "peerDependencies", deps);
    }
    if let Some(meta) = extension.peer_dependencies_meta.as_ref() {
        merge_peer_meta(manifest, meta);
    }
}

fn merge_string_map<Key, Value_>(
    manifest: &mut Map<String, Value>,
    key: &str,
    extension_map: &std::collections::BTreeMap<Key, Value_>,
) where
    Key: AsRef<str>,
    Value_: AsRef<str>,
{
    let existing = manifest
        .remove(key)
        .and_then(|value| if let Value::Object(map) = value { Some(map) } else { None })
        .unwrap_or_default();
    let mut merged: Map<String, Value> = Map::new();
    for (name, value) in extension_map {
        merged.insert(name.as_ref().to_string(), Value::String(value.as_ref().to_string()));
    }
    for (name, value) in existing {
        merged.insert(name, value);
    }
    manifest.insert(key.to_string(), Value::Object(merged));
}

fn merge_peer_meta(
    manifest: &mut Map<String, Value>,
    extension_meta: &std::collections::BTreeMap<String, pacquet_config::PeerDependencyMeta>,
) {
    let existing = manifest
        .remove("peerDependenciesMeta")
        .and_then(|value| if let Value::Object(map) = value { Some(map) } else { None })
        .unwrap_or_default();
    let mut merged: Map<String, Value> = Map::new();
    for (name, meta) in extension_meta {
        let mut entry = Map::new();
        if let Some(optional) = meta.optional {
            entry.insert("optional".to_string(), Value::Bool(optional));
        }
        merged.insert(name.clone(), Value::Object(entry));
    }
    for (name, value) in existing {
        merged.insert(name, value);
    }
    manifest.insert("peerDependenciesMeta".to_string(), Value::Object(merged));
}

#[cfg(test)]
mod tests;
