//! Apply parsed `pnpm.overrides` to a `PackageManifest` before
//! downstream consumers read its dependency maps.
//!
//! Pacquet port of upstream's
//! [`createVersionsOverrider`](https://github.com/pnpm/pnpm/blob/0d88df854f/hooks/read-package-hook/src/createVersionsOverrider.ts).
//! Upstream's hook is a `readPackageHook` that fires on every manifest
//! read during resolution. Pacquet uses the same rewrite both for
//! frozen-lockfile freshness checks and for the fresh resolver's
//! manifest hook. The shape of the rewrite — generic vs.
//! parent-scoped overrides, `-` deletion, `link:` / `file:` local
//! targets, range intersection via semver — is preserved so the
//! resolved dependency graph and lockfile match pnpm's post-override
//! manifest view.
//!
//! What's intentionally **not** ported yet:
//! - `peerDependencies` mutation. Upstream promotes a peer to
//!   `dependencies` when the new specifier isn't a valid peer range,
//!   and writes a valid peer-range spec back to `peerDependencies`.
//!   Pacquet's freshness check doesn't read `peerDependencies` and
//!   pacquet has no peer install yet, so a frozen install on a repo
//!   with peer overrides doesn't need the rewrite to stay correct
//!   today. When peer install lands, the peer arm gets ported with
//!   it.
//!
//! The hook never touches the on-disk `package.json` — mutation
//! happens through [`pacquet_package_manifest::PackageManifest::value_mut`]
//! on the in-memory `Value` only.

use node_semver::{Range, Version};
use pacquet_config_parse_overrides::{PackageSelector, VersionOverride};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// In-memory hook that applies the parsed `pnpm.overrides` set to a
/// manifest. Cheap to construct — partitioning the overrides into the
/// parent-scoped vs. generic buckets happens once, and each call to
/// [`Self::apply`] walks the dep maps in place.
pub struct VersionsOverrider {
    /// Overrides whose key carries a `parent>child` shape — only
    /// applied when the manifest being rewritten matches the parent
    /// half (name + optional range).
    parent_scoped: Vec<ResolvedOverride>,
    /// Generic overrides (no parent half). Apply to every manifest.
    generic: Vec<ResolvedOverride>,
}

/// `VersionOverride` augmented with a pre-parsed `LocalTarget` for
/// the local-protocol forms. Splitting once at construction time
/// avoids re-parsing the prefix on every manifest read.
struct ResolvedOverride {
    inner: VersionOverride,
    local_target: Option<LocalTarget>,
}

#[derive(Debug, Clone, Copy)]
enum LocalProtocol {
    Link,
    File,
}

impl LocalProtocol {
    fn as_str(self) -> &'static str {
        match self {
            LocalProtocol::Link => "link:",
            LocalProtocol::File => "file:",
        }
    }
}

struct LocalTarget {
    protocol: LocalProtocol,
    absolute_path: PathBuf,
    specified_via_relative_path: bool,
}

impl VersionsOverrider {
    /// Build the hook from the parsed overrides set produced by
    /// [`pacquet_config_parse_overrides::parse_overrides`].
    #[must_use]
    pub fn new(overrides: &[VersionOverride], root_dir: &Path) -> Self {
        let mut parent_scoped = Vec::new();
        let mut generic = Vec::new();
        for override_entry in overrides {
            let resolved = ResolvedOverride {
                inner: override_entry.clone(),
                local_target: parse_local_target(&override_entry.new_bare_specifier, root_dir),
            };
            if override_entry.parent_pkg.is_some() {
                parent_scoped.push(resolved);
            } else {
                generic.push(resolved);
            }
        }
        VersionsOverrider { parent_scoped, generic }
    }

    /// `true` when the hook has no entries and can be skipped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parent_scoped.is_empty() && self.generic.is_empty()
    }

    /// Apply the override set to `manifest` in place. `manifest_dir`
    /// is the directory containing the manifest (used as the base for
    /// re-relativizing `link:` / `file:` overrides that were
    /// specified relative to the `root_dir` passed to
    /// [`Self::new`]). For the root project manifest,
    /// `manifest_dir == Some(root_dir)`.
    ///
    /// Mirrors upstream's `(manifest, dir) => { ... }` body returned
    /// by `createVersionsOverrider`.
    pub fn apply(&self, manifest: &mut PackageManifest, manifest_dir: Option<&Path>) {
        self.apply_to_value(manifest.value_mut(), manifest_dir);
    }

    /// Apply the override set to a manifest JSON value in place.
    pub fn apply_to_value(&self, manifest: &mut Value, manifest_dir: Option<&Path>) {
        let applicable_parent_scoped = self.applicable_parent_scoped(manifest);

        // Upstream's `overrideDepsOfPkg` mutates the four dep buckets
        // (`dependencies`, `optionalDependencies`, `devDependencies`,
        // and the `peerDependencies → dependencies` migration).
        // Pacquet today skips the peer arm — see the module-level doc
        // for why.
        for group in
            [DependencyGroup::Prod, DependencyGroup::Optional, DependencyGroup::Dev].iter().copied()
        {
            self.override_group(manifest, group, &applicable_parent_scoped, manifest_dir);
        }
    }

    /// Apply overrides to the resolver's shared manifest value,
    /// cloning only when at least one configured override can rewrite
    /// the manifest.
    #[must_use]
    pub fn apply_to_arc(&self, manifest: Arc<Value>, manifest_dir: Option<&Path>) -> Arc<Value> {
        if self.is_empty() || !self.has_applicable_override(&manifest) {
            return manifest;
        }
        let mut cloned = (*manifest).clone();
        self.apply_to_value(&mut cloned, manifest_dir);
        Arc::new(cloned)
    }

    fn applicable_parent_scoped<'b>(&'b self, manifest: &Value) -> Vec<&'b ResolvedOverride> {
        let manifest_name = manifest.get("name").and_then(Value::as_str);
        let manifest_version = manifest.get("version").and_then(Value::as_str);

        self.parent_scoped
            .iter()
            .filter(|entry| {
                let Some(parent) = entry.inner.parent_pkg.as_ref() else { return false };
                let name_matches = manifest_name == Some(parent.name.as_str());
                let range_matches = match (parent.bare_specifier.as_deref(), manifest_version) {
                    (None, _) => true,
                    (Some(_), None) => false,
                    (Some(range), Some(version)) => semver_satisfies(version, range),
                };
                name_matches && range_matches
            })
            .collect()
    }

    fn has_applicable_override(&self, value: &Value) -> bool {
        let applicable_parent_scoped = self.applicable_parent_scoped(value);
        [DependencyGroup::Prod, DependencyGroup::Optional, DependencyGroup::Dev]
            .iter()
            .copied()
            .any(|group| self.group_has_override(value, group, &applicable_parent_scoped))
    }

    fn group_has_override(
        &self,
        value: &Value,
        group: DependencyGroup,
        applicable_parent_scoped: &[&ResolvedOverride],
    ) -> bool {
        let key: &'static str = group.into();
        let Some(map) = value.get(key).and_then(Value::as_object) else { return false };

        map.iter().any(|(name, spec)| {
            spec.as_str().is_some_and(|spec| {
                self.choose_override(applicable_parent_scoped, name, spec).is_some()
            })
        })
    }

    fn override_group(
        &self,
        value: &mut Value,
        group: DependencyGroup,
        applicable_parent_scoped: &[&ResolvedOverride],
        manifest_dir: Option<&Path>,
    ) {
        let key: &'static str = group.into();
        let Some(map) = value.get_mut(key).and_then(Value::as_object_mut) else { return };

        let entries: Vec<(String, String)> = map
            .iter()
            .filter_map(|(name, spec)| {
                spec.as_str().map(|spec_str| (name.clone(), spec_str.to_string()))
            })
            .collect();

        for (name, spec) in entries {
            let Some(chosen) = self.choose_override(applicable_parent_scoped, &name, &spec) else {
                continue;
            };

            if chosen.inner.new_bare_specifier == "-" {
                map.remove(&name);
                continue;
            }

            let new_spec = chosen.local_target.as_ref().map_or_else(
                || chosen.inner.new_bare_specifier.clone(),
                |target| resolve_local_override_spec(target, manifest_dir),
            );

            map.insert(name, Value::String(new_spec));
        }
    }

    fn choose_override<'b>(
        &'b self,
        applicable_parent_scoped: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        Self::pick_most_specific(applicable_parent_scoped, dep_name, dep_spec)
            .or_else(|| self.pick_most_specific_generic(dep_name, dep_spec))
    }

    fn pick_most_specific<'b>(
        candidates: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        let mut matching: Vec<&'b ResolvedOverride> = candidates
            .iter()
            .copied()
            .filter(|entry| matches_target(&entry.inner.target_pkg, dep_name, dep_spec))
            .collect();
        sort_by_specificity(&mut matching);
        matching.into_iter().next()
    }

    fn pick_most_specific_generic(
        &self,
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&ResolvedOverride> {
        let mut matching: Vec<&ResolvedOverride> = self
            .generic
            .iter()
            .filter(|entry| matches_target(&entry.inner.target_pkg, dep_name, dep_spec))
            .collect();
        sort_by_specificity(&mut matching);
        matching.into_iter().next()
    }
}

/// Mirrors upstream's `targetPkg.name === name && isIntersectingRange(targetPkg.bareSpecifier, bareSpecifier)`.
fn matches_target(target: &PackageSelector, dep_name: &str, dep_spec: &str) -> bool {
    target.name == dep_name && is_intersecting_range(target.bare_specifier.as_deref(), dep_spec)
}

/// Sort overrides so the "most specific" one — the one whose target
/// range is contained inside the others — sorts first. Mirrors
/// upstream's
/// [`pickMostSpecificVersionOverride`](https://github.com/pnpm/pnpm/blob/0d88df854f/hooks/read-package-hook/src/createVersionsOverrider.ts#L137-L139):
/// `sort((a, b) => isIntersectingRange(b.targetPkg.bareSpecifier ?? '', a.targetPkg.bareSpecifier ?? '') ? -1 : 1)[0]`.
/// The intuition is `b ⊃ a ⇒ a sorts before b`, so a narrower target
/// like `foo@1.2.3` wins over the broader `foo@^1`.
fn sort_by_specificity(matching: &mut [&ResolvedOverride]) {
    matching.sort_by(|lhs, rhs| {
        let lhs_spec = lhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
        let rhs_spec = rhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
        // Upstream's comparator (`? -1 : 1`) only emits Less/Greater
        // and relies on V8's lenient sort. Rust's `sort_by` requires
        // a total order, so we widen to a 3-way result: `lhs` is
        // strictly more specific when `rhs ⊇ lhs` but not vice versa,
        // strictly less specific in the mirror case, and equal when
        // both ranges cover each other (e.g. identical strings, or
        // mutually-intersecting unions). The strict cases keep the
        // sort outcome identical to upstream's first match; the
        // `Equal` arm is what keeps `sort_by`'s preconditions
        // satisfied.
        let rhs_covers_lhs = is_intersecting_range(Some(rhs_spec), lhs_spec);
        let lhs_covers_rhs = is_intersecting_range(Some(lhs_spec), rhs_spec);
        match (rhs_covers_lhs, lhs_covers_rhs) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
}

/// Mirrors upstream's
/// [`isIntersectingRange`](https://github.com/pnpm/pnpm/blob/bcc8eb2622/hooks/read-package-hook/src/isIntersectingRange.ts).
///
/// An absent `range1` (or empty target spec) matches everything. An
/// exact-equal range pair matches without parsing. Otherwise both
/// sides must parse as semver and have a non-empty intersection.
fn is_intersecting_range(range1: Option<&str>, range2: &str) -> bool {
    let Some(range1_str) = range1 else { return true };
    if range1_str.is_empty() || range2 == range1_str {
        return true;
    }
    let Ok(parsed1) = range1_str.parse::<Range>() else { return false };
    let Ok(parsed2) = range2.parse::<Range>() else { return false };
    parsed1.allows_any(&parsed2)
}

/// True when `version` (parsed as a concrete semver) satisfies
/// `range`. Mirrors upstream's `semver.satisfies(manifest.version,
/// parentPkg.bareSpecifier)` guard in the parent-scoped filter.
/// A non-parseable version OR range fails the match conservatively —
/// the parent constraint is treated as not applying.
fn semver_satisfies(version: &str, range: &str) -> bool {
    let Ok(parsed_version) = version.parse::<Version>() else { return false };
    let Ok(parsed_range) = range.parse::<Range>() else { return false };
    parsed_range.satisfies(&parsed_version)
}

/// Parse the override's `new_bare_specifier` for the `link:` / `file:`
/// prefix. Returns `None` for any other shape — semver ranges, tarball
/// URLs, npm-alias specs, etc.
///
/// Mirrors upstream's
/// [`createLocalTarget`](https://github.com/pnpm/pnpm/blob/0d88df854f/hooks/read-package-hook/src/createVersionsOverrider.ts#L45-L58).
fn parse_local_target(new_bare_specifier: &str, root_dir: &Path) -> Option<LocalTarget> {
    let (protocol, pkg_path) = if let Some(rest) = new_bare_specifier.strip_prefix("file:") {
        (LocalProtocol::File, rest)
    } else if let Some(rest) = new_bare_specifier.strip_prefix("link:") {
        (LocalProtocol::Link, rest)
    } else {
        return None;
    };

    let candidate = Path::new(pkg_path);
    let specified_via_relative_path = !candidate.is_absolute();
    let absolute_path = if specified_via_relative_path {
        root_dir.join(candidate)
    } else {
        candidate.to_path_buf()
    };
    Some(LocalTarget { protocol, absolute_path, specified_via_relative_path })
}

/// Render a `link:` / `file:` override against the importing
/// package's directory. Relative-form targets are re-anchored against
/// `pkg_dir` so they read sensibly from the consumer's perspective;
/// absolute-form targets are emitted verbatim. Mirrors upstream's
/// [`resolveLocalOverride`](https://github.com/pnpm/pnpm/blob/0d88df854f/hooks/read-package-hook/src/createVersionsOverrider.ts#L131-L135).
fn resolve_local_override_spec(target: &LocalTarget, pkg_dir: Option<&Path>) -> String {
    // Every branch routes through `normalize_path` so absolute and
    // diff-paths-fallback shapes also get backslash → forward-slash
    // rewriting on Windows; upstream's `link:` / `file:` specifiers
    // must use forward slashes regardless of host OS.
    let path_str = match (target.specified_via_relative_path, pkg_dir) {
        (true, Some(dir)) => pathdiff::diff_paths(&target.absolute_path, dir)
            .as_deref()
            .map_or_else(|| normalize_path(&target.absolute_path), normalize_path),
        _ => normalize_path(&target.absolute_path),
    };
    format!("{}{path_str}", target.protocol.as_str())
}

/// Replace `\\` with `/` to match upstream's `normalize-path` step.
/// `link:` / `file:` specifiers must use forward slashes regardless
/// of host OS — pnpm's lockfile and pacquet's downstream consumers
/// expect that shape.
fn normalize_path(path: &Path) -> String {
    let display = path.display().to_string();
    if cfg!(windows) { display.replace('\\', "/") } else { display }
}

#[cfg(test)]
mod tests;
