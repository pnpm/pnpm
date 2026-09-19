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
//! happens through [`pnpm_package_manifest::PackageManifest::value_mut`]
//! on the in-memory `Value` only.

pub(crate) use selectors::parse_declared_range;

mod selectors;
use selectors::{matches_target, semver_satisfies, sort_by_specificity};

use node_semver::{Range, Version};
use pnpm_config_parse_overrides::{PackageSelector, VersionOverride};
use pnpm_local_spec::LocalSpec;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::is_valid_peer_range;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
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
}

/// A convergence override's replacement value, with the exact version
/// pre-parsed once at construction time. `version` is `None` when the
/// value isn't a parseable semver version (only reachable for
/// hand-built [`VersionOverride`] entries — [`pnpm_config_parse_overrides::parse_overrides`]
/// rejects such values); the override then never rewrites an edge,
/// matching how an unsatisfiable version behaves.
struct ConvergeOverride {
    new_bare_specifier: String,
    version: Option<Version>,
}

/// `VersionOverride` augmented with a pre-parsed [`LocalSpec`] for
/// the local-protocol forms. Splitting once at construction time
/// avoids re-parsing the prefix on every manifest read.
struct ResolvedOverride {
    inner: VersionOverride,
    local_target: Option<LocalSpec>,
}

/// Answers whether an override governs a dependency declared as a given
/// specifier — whether or not it rewrites the text. An override that repeats
/// the declaration verbatim still governs it, and a range-scoped override
/// (`foo@^2`) governs one declaration of `foo` and not another, so the
/// declared specifier is part of the question.
///
/// Built by [`VersionsOverrider::dependency_matcher`] for one manifest.
pub struct OverriddenDependencyMatcher<'a> {
    overrider: &'a VersionsOverrider,
    applicable_parent_scoped: Vec<&'a ResolvedOverride>,
}

impl OverriddenDependencyMatcher<'_> {
    #[must_use]
    pub fn matches(&self, dep_name: &str, dep_spec: &str) -> bool {
        self.overrider.choose_override(&self.applicable_parent_scoped, dep_name, dep_spec).is_some()
            || self.overrider.converge_applies(dep_name, dep_spec)
    }
}

impl VersionsOverrider {
    /// Build the hook from the parsed overrides set produced by
    /// [`pnpm_config_parse_overrides::parse_overrides`].
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
                local_target: LocalSpec::parse(&override_entry.new_bare_specifier, root_dir),
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
        }
    }

    /// `true` when the hook has no entries and can be skipped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parent_scoped.is_empty()
            && self.generic.is_empty()
            && self.converge.is_empty()
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

        map.iter()
            .any(|(name, spec)| {
                spec.as_str()
                    .is_some_and(|spec| {
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
                spec.as_str()
                    .map(|spec_str| (name.clone(), spec_str.to_string()))
            })
            .collect();

        for (name, spec) in entries {
            let Some(chosen) = self.choose_override(applicable_parent_scoped, &name, &spec) else {
                if let Some(new_spec) = self.converge_dep(&name, &spec) {
                    map.insert(name, Value::String(new_spec));
                }
                continue;
            };

            if chosen.inner.new_bare_specifier == "-" {
                map.remove(&name);
                continue;
            }

            let new_spec = chosen.local_target
                .as_ref()
                .map_or_else(
                    || chosen.inner.new_bare_specifier.clone(),
                    |target| target.render(manifest_dir),
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
                        spec.as_str()
                            .map(|spec_str| (name.clone(), spec_str.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (name, spec) in entries {
            self.override_peer_entry(value, applicable_parent_scoped, manifest_dir, name, &spec);
        }
    }

    fn override_peer_entry(
        &self,
        value: &mut Value,
        applicable_parent_scoped: &[&ResolvedOverride],
        manifest_dir: Option<&Path>,
        name: String,
        spec: &str,
    ) {
        let Some(chosen) = self.choose_override(applicable_parent_scoped, &name, spec) else {
            // A convergence override's value is an exact version —
            // always a valid peer range — so the rewrite stays in
            // `peerDependencies`.
            if let Some(new_spec) = self.converge_dep(&name, spec) {
                insert_peer_dependency(value, name, new_spec);
            }
            return;
        };
        if chosen.inner.new_bare_specifier == "-" {
            remove_peer_dependency(value, &name);
            return;
        }
        let new_spec = chosen.local_target
            .as_ref()
            .map_or_else(
                || chosen.inner.new_bare_specifier.clone(),
                |target| target.render(manifest_dir),
            );
        if is_valid_peer_range(&new_spec) {
            insert_peer_dependency(value, name, new_spec);
            return;
        }
        insert_regular_dependency(value, name, new_spec);
    }

    /// Resolve the specifier the override set imposes on a dependency
    /// edge that has no declaring manifest — a peer pnpm auto-installs.
    /// `"-"` means the edge is dropped. Parent-scoped overrides never
    /// apply: there is no parent manifest to match them against.
    /// `pkg_dir` is the directory of the package the edge is added to, so
    /// a `link:` / `file:` target stays relative to it instead of
    /// hard-coding this machine's layout into the lockfile.
    ///
    /// Such an edge never reaches [`Self::apply`], so the convergence
    /// collector must not see it either — a range no manifest declares
    /// would skew the staleness verdict.
    #[must_use]
    pub fn override_for_undeclared_dependency(
        &self,
        dep_name: &str,
        dep_spec: &str,
        pkg_dir: &Path,
    ) -> Option<String> {
        if let Some(chosen) = self.choose_override(&[], dep_name, dep_spec) {
            if chosen.inner.new_bare_specifier == "-" {
                return Some("-".to_string());
            }
            return Some(
                chosen.local_target
                    .as_ref()
                    .map_or_else(
                        || chosen.inner.new_bare_specifier.clone(),
                        |target| target.render(Some(pkg_dir)),
                    ),
            );
        }
        self.converge_applies(dep_name, dep_spec)
            .then(|| self.converge[dep_name].new_bare_specifier.clone())
    }

    /// An [`OverriddenDependencyMatcher`] bound to `manifest`, so the
    /// parent-scoped overrides that manifest answers to are selected once
    /// rather than once per dependency asked about.
    #[must_use]
    pub fn dependency_matcher<'a>(&'a self, manifest: &Value) -> OverriddenDependencyMatcher<'a> {
        OverriddenDependencyMatcher {
            overrider: self,
            applicable_parent_scoped: self.applicable_parent_scoped(manifest),
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
        let mut matching: Vec<&ResolvedOverride> = self.generic
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
        self.converge
            .get(dep_name)
            .is_some_and(|entry| {
                entry.version
                    .as_ref()
                    .is_some_and(|version| {
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

/// Rewrite a peer's range, as long as the manifest still declares a
/// `peerDependencies` object.
fn insert_peer_dependency(value: &mut Value, name: String, spec: String) {
    if let Some(peers) = value.get_mut("peerDependencies").and_then(Value::as_object_mut) {
        peers.insert(name, Value::String(spec));
    }
}

fn remove_peer_dependency(value: &mut Value, name: &str) {
    if let Some(peers) = value.get_mut("peerDependencies").and_then(Value::as_object_mut) {
        peers.remove(name);
    }
}

/// An override value that is not a valid peer range moves the edge into
/// `dependencies`, creating that object when the manifest declares none.
fn insert_regular_dependency(value: &mut Value, name: String, spec: String) {
    if !value.get("dependencies").is_some_and(Value::is_object)
        && let Some(root) = value.as_object_mut()
    {
        root.insert("dependencies".to_string(), Value::Object(serde_json::Map::new()));
    }
    if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
        deps.insert(name, Value::String(spec));
    }
}

#[cfg(test)]
mod tests;
