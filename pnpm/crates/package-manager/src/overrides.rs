//! Apply parsed `pnpm.overrides` to a `PackageManifest` before
//! downstream consumers read its dependency maps.
//!
//! The rewrite is a manifest hook that fires on every manifest
//! read during resolution. Pacquet uses the same rewrite both for
//! frozen-lockfile freshness checks and for the fresh resolver's
//! manifest hook. Its shape — generic vs.
//! parent-scoped overrides, `-` deletion, `link:` / `file:` local
//! targets, range intersection via semver — drives both the
//! resolved dependency graph and the lockfile's post-override
//! manifest view.
//!
//! The hook never touches the on-disk `package.json` — mutation
//! happens through [`pacquet_package_manifest::PackageManifest::value_mut`]
//! on the in-memory `Value` only.

use node_semver::{Range, Version};
use pacquet_config_parse_overrides::{PackageSelector, VersionOverride};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::is_valid_peer_range;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// In-memory hook that applies the parsed `pnpm.overrides` set to a
/// manifest. Cheap to construct — partitioning the overrides into the
/// parent-scoped vs. generic vs. convergence buckets happens once, and
/// each call to [`Self::apply`] walks the dep maps in place.
pub struct VersionsOverrider {
    /// Overrides whose key carries a `parent>child` shape — only
    /// applied when the manifest being rewritten matches the parent
    /// half (name + optional range).
    parent_scoped: Vec<ResolvedOverride>,
    /// Generic overrides (no parent half). Apply to every manifest.
    generic: Vec<ResolvedOverride>,
    /// Convergence overrides (`"pkg@"`), keyed by target package name
    /// (at most one per name — the overrides map's keys are unique).
    /// Consulted only for edges no explicit override claims.
    converge: HashMap<String, ConvergeOverride>,
    /// Every declared semver range seen for packages that have a
    /// convergence override, whether or not the override's version
    /// satisfied it. Feeds the staleness check for convergence
    /// overrides after a full resolution. Edges claimed by an explicit
    /// override are not recorded — the convergence override never
    /// governs them.
    converge_declared_ranges: Mutex<HashMap<String, HashSet<String>>>,
    /// Selectors that have been observed to match at least one
    /// manifest passed through `apply*`. Mirrors pnpm's
    /// `appliedOverrides` Set threaded via `onApplied` in
    /// `createVersionsOverrider`; readers (the post-resolution
    /// unused-override check) call [`Self::applied_selectors`] to
    /// compute the diff against the configured set.
    applied: Arc<Mutex<HashSet<String>>>,
}

/// A convergence override's replacement value, with the exact version
/// pre-parsed once at construction time. `version` is `None` when the
/// value isn't a parseable semver version (only reachable for
/// hand-built [`VersionOverride`] entries — [`pacquet_config_parse_overrides::parse_overrides`]
/// rejects such values); the override then never rewrites an edge,
/// matching how an unsatisfiable version behaves.
struct ConvergeOverride {
    new_bare_specifier: String,
    version: Option<Version>,
}

/// `VersionOverride` augmented with a pre-parsed [`LocalTarget`] for
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
        let mut converge = HashMap::new();
        for override_entry in overrides {
            if override_entry.converge {
                converge.insert(
                    override_entry.target_pkg.name.clone(),
                    ConvergeOverride {
                        new_bare_specifier: override_entry.new_bare_specifier.clone(),
                        version: Version::parse(&override_entry.new_bare_specifier).ok(),
                    },
                );
                continue;
            }
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
        VersionsOverrider {
            parent_scoped,
            generic,
            converge,
            converge_declared_ranges: Mutex::new(HashMap::new()),
            applied: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// `true` when the hook has no entries and can be skipped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parent_scoped.is_empty() && self.generic.is_empty() && self.converge.is_empty()
    }

    /// Snapshot of every declared range recorded so far for packages
    /// governed by a convergence override. Read by the staleness check
    /// after a full resolution has streamed every manifest through
    /// this hook.
    #[must_use]
    pub fn converge_declared_ranges(&self) -> HashMap<String, HashSet<String>> {
        self.converge_declared_ranges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Snapshot of the selectors that matched at least one manifest
    /// passed through `apply*` since construction. Mirrors pnpm's
    /// `appliedOverrides` Set content at the moment the
    /// post-resolution verifier runs.
    #[must_use]
    pub fn applied_selectors(&self) -> HashSet<String> {
        self.applied.lock().expect("applied overrides mutex not poisoned").clone()
    }

    /// Apply the override set to `manifest` in place. `manifest_dir`
    /// is the directory containing the manifest (used as the base for
    /// re-relativizing `link:` / `file:` overrides that were
    /// specified relative to the `root_dir` passed to
    /// [`Self::new`]). For the root project manifest,
    /// `manifest_dir == Some(root_dir)`.
    pub fn apply(&self, manifest: &mut PackageManifest, manifest_dir: Option<&Path>) {
        self.apply_to_value(manifest.value_mut(), manifest_dir);
    }

    /// Apply the override set to a manifest JSON value in place.
    pub fn apply_to_value(&self, manifest: &mut Value, manifest_dir: Option<&Path>) {
        let applicable_parent_scoped = self.applicable_parent_scoped(manifest);

        for group in
            [DependencyGroup::Prod, DependencyGroup::Optional, DependencyGroup::Dev].iter().copied()
        {
            self.override_group(manifest, group, &applicable_parent_scoped, manifest_dir);
        }
        self.override_peer_group(manifest, &applicable_parent_scoped, manifest_dir);
    }

    /// Apply overrides to the resolver's shared manifest value,
    /// cloning only when at least one configured override can rewrite
    /// the manifest.
    #[must_use]
    pub fn apply_to_arc(&self, manifest: Arc<Value>, manifest_dir: Option<&Path>) -> Arc<Value> {
        if self.is_empty() {
            return manifest;
        }
        if !self.has_applicable_override(&manifest) {
            // Nothing rewrites, so the shared value is returned as-is —
            // but the declared ranges of converge-governed edges must
            // still reach the staleness collector (`apply_to_value`
            // records them as a side effect of its walk, which is
            // skipped here).
            self.record_converge_declared_ranges(&manifest);
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
        [
            DependencyGroup::Prod,
            DependencyGroup::Optional,
            DependencyGroup::Dev,
            DependencyGroup::Peer,
        ]
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
                    || self.converge_applies(name, spec)
            })
        })
    }

    /// Record the declared ranges of every converge-governed edge in
    /// `value`, without rewriting anything. Same claimed-edge exclusion
    /// as the rewrite walk: an edge an explicit override picks up is
    /// never governed by the convergence override, so its range does
    /// not participate in the staleness verdict.
    fn record_converge_declared_ranges(&self, value: &Value) {
        if self.converge.is_empty() {
            return;
        }
        let applicable_parent_scoped = self.applicable_parent_scoped(value);
        for group in [
            DependencyGroup::Prod,
            DependencyGroup::Optional,
            DependencyGroup::Dev,
            DependencyGroup::Peer,
        ] {
            let key: &'static str = group.into();
            let Some(map) = value.get(key).and_then(Value::as_object) else { continue };
            for (name, spec) in map {
                let Some(spec) = spec.as_str() else { continue };
                if self.choose_override(&applicable_parent_scoped, name, spec).is_none() {
                    self.try_record_converge_range(name, spec);
                }
            }
        }
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
                if let Some(new_spec) = self.converge_dep(&name, &spec) {
                    map.insert(name, Value::String(new_spec));
                }
                continue;
            };

            self.record_applied(chosen);

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

    fn override_peer_group(
        &self,
        value: &mut Value,
        applicable_parent_scoped: &[&ResolvedOverride],
        manifest_dir: Option<&Path>,
    ) {
        let entries: Vec<(String, String)> = value
            .get("peerDependencies")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(name, spec)| {
                        spec.as_str().map(|spec_str| (name.clone(), spec_str.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (name, spec) in entries {
            let Some(chosen) = self.choose_override(applicable_parent_scoped, &name, &spec) else {
                // A convergence override's value is an exact version —
                // always a valid peer range — so the rewrite stays in
                // `peerDependencies`.
                if let Some(new_spec) = self.converge_dep(&name, &spec)
                    && let Some(peers) =
                        value.get_mut("peerDependencies").and_then(Value::as_object_mut)
                {
                    peers.insert(name, Value::String(new_spec));
                }
                continue;
            };

            self.record_applied(chosen);

            if chosen.inner.new_bare_specifier == "-" {
                if let Some(peers) =
                    value.get_mut("peerDependencies").and_then(Value::as_object_mut)
                {
                    peers.remove(&name);
                }
                continue;
            }

            let new_spec = chosen.local_target.as_ref().map_or_else(
                || chosen.inner.new_bare_specifier.clone(),
                |target| resolve_local_override_spec(target, manifest_dir),
            );

            if is_valid_peer_range(&new_spec) {
                if let Some(peers) =
                    value.get_mut("peerDependencies").and_then(Value::as_object_mut)
                {
                    peers.insert(name, Value::String(new_spec));
                }
            } else {
                if !value.get("dependencies").is_some_and(Value::is_object)
                    && let Some(root) = value.as_object_mut()
                {
                    root.insert("dependencies".to_string(), Value::Object(serde_json::Map::new()));
                }
                if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
                    deps.insert(name, Value::String(new_spec));
                }
            }
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

    /// Record a hit on `chosen` so the post-resolution
    /// unused-override verifier can tell it apart from overrides that
    /// never matched. The selector stored is the raw override key
    /// (`foo`, `parent>child`, `foo@^1`); pnpm uses the same value
    /// for its diff. Checked before cloning to avoid repeated
    /// allocations when the same selector matches many manifests.
    fn record_applied(&self, chosen: &ResolvedOverride) {
        let mut guard = self.applied.lock().expect("applied overrides mutex not poisoned");
        if !guard.contains(&chosen.inner.selector) {
            guard.insert(chosen.inner.selector.clone());
        }
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

    /// Consult the convergence override for an edge no explicit
    /// override claimed: record the declared range for the staleness
    /// check, then return the exact version when it satisfies the
    /// declared range — incompatible edges keep their own resolution.
    fn converge_dep(&self, dep_name: &str, dep_spec: &str) -> Option<String> {
        let range = self.try_record_converge_range(dep_name, dep_spec)?;
        let entry = &self.converge[dep_name];
        let version = entry.version.as_ref()?;
        range.satisfies(version).then(|| entry.new_bare_specifier.clone())
    }

    /// Rewrite-only variant of [`Self::converge_dep`] for
    /// [`Self::has_applicable_override`]'s clone gate: same verdict,
    /// no collector side effect.
    fn converge_applies(&self, dep_name: &str, dep_spec: &str) -> bool {
        self.converge.get(dep_name).is_some_and(|entry| {
            entry.version.as_ref().is_some_and(|version| {
                parse_declared_range(dep_spec).is_some_and(|range| range.satisfies(version))
            })
        })
    }

    /// When `dep_name` is converge-governed and `dep_spec` is a plain
    /// semver range, record the range into the staleness collector and
    /// return it parsed. `None` skips the edge entirely.
    fn try_record_converge_range(&self, dep_name: &str, dep_spec: &str) -> Option<Range> {
        self.converge.get(dep_name)?;
        let range = parse_declared_range(dep_spec)?;
        self.converge_declared_ranges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(dep_name.to_string())
            .or_default()
            .insert(dep_spec.to_string());
        Some(range)
    }
}

/// Parse a dependency edge's declared spec for the convergence
/// consult. Only plain semver ranges participate — `workspace:`,
/// `catalog:`, `npm:`, git/URL, and dist-tag specifiers have no
/// defined "satisfies" relation and yield `None`. An empty declared
/// spec counts as `*`.
pub(crate) fn parse_declared_range(spec: &str) -> Option<Range> {
    if spec.is_empty() {
        return Some(Range::any());
    }
    Range::parse(spec).ok()
}

/// A target matches when its name equals `dep_name` and its range
/// intersects `dep_spec`.
fn matches_target(target: &PackageSelector, dep_name: &str, dep_spec: &str) -> bool {
    target.name == dep_name && is_intersecting_range(target.bare_specifier.as_deref(), dep_spec)
}

/// Sort overrides so the "most specific" one — the one whose target
/// range is contained inside the others — sorts first.
/// The intuition is `b ⊃ a ⇒ a sorts before b`, so a narrower target
/// like `foo@1.2.3` wins over the broader `foo@^1`.
fn sort_by_specificity(matching: &mut [&ResolvedOverride]) {
    matching.sort_by(|lhs, rhs| {
        let lhs_spec = lhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
        let rhs_spec = rhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
        // Rust's `sort_by` requires a total order, so the comparison
        // widens to a 3-way result: `lhs` is
        // strictly more specific when `rhs ⊇ lhs` but not vice versa,
        // strictly less specific in the mirror case, and equal when
        // both ranges cover each other (e.g. identical strings, or
        // mutually-intersecting unions). The `Equal` arm is what keeps
        // `sort_by`'s preconditions satisfied.
        let rhs_covers_lhs = is_intersecting_range(Some(rhs_spec), lhs_spec);
        let lhs_covers_rhs = is_intersecting_range(Some(lhs_spec), rhs_spec);
        match (rhs_covers_lhs, lhs_covers_rhs) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
}

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
/// `range`, the guard in the parent-scoped filter.
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
fn parse_local_target(new_bare_specifier: &str, root_dir: &Path) -> Option<LocalTarget> {
    let (protocol, pkg_path) = if let Some(rest) = new_bare_specifier.strip_prefix("file:") {
        (LocalProtocol::File, rest)
    } else {
        (LocalProtocol::Link, new_bare_specifier.strip_prefix("link:")?)
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
/// absolute-form targets are emitted verbatim.
fn resolve_local_override_spec(target: &LocalTarget, pkg_dir: Option<&Path>) -> String {
    // Every branch routes through `normalize_path` so absolute and
    // diff-paths-fallback shapes also get backslash → forward-slash
    // rewriting on Windows; `link:` / `file:` specifiers
    // must use forward slashes regardless of host OS.
    let path_str = match (target.specified_via_relative_path, pkg_dir) {
        (true, Some(dir)) => pathdiff::diff_paths(&target.absolute_path, dir)
            .as_deref()
            .map_or_else(|| normalize_path(&target.absolute_path), normalize_path),
        _ => normalize_path(&target.absolute_path),
    };
    format!("{}{path_str}", target.protocol.as_str())
}

/// Replace `\\` with `/` to normalize the path.
/// `link:` / `file:` specifiers must use forward slashes regardless
/// of host OS — the lockfile and pacquet's downstream consumers
/// expect that shape.
fn normalize_path(path: &Path) -> String {
    let display = path.display().to_string();
    if cfg!(windows) { display.replace('\\', "/") } else { display }
}

#[cfg(test)]
mod tests;
