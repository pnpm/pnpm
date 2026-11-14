use crate::{
    features::{locked_dependencies, supports_features},
    metadata::{parse_metadata, root_dependencies},
    model::{
        CancelError, FeatureSelection, PackageKey, RegistryDependency, RegistryVersion,
        ensure_not_cancelled,
    },
    preferences::{PreferredVersions, contains as is_preferred, parse as parse_preferred_versions},
    registry::{Registry, compatibility_line},
};
use miette::Result;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// What discovery has reached a crate with so far: the features every
/// dependency edge has asked of it together, and the versions those edges
/// can select.
#[derive(Default)]
struct Discovered {
    selection: FeatureSelection,
    versions: BTreeSet<Version>,
}

/// Return sparse-index package names still needed to resolve `metadata`
/// against the registry identified by `source`.
pub fn missing_index_names(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
) -> Result<Vec<String>> {
    let mut discovery = IndexDiscovery::new(metadata, source)?;
    discovery.add_entries(index_files)?;
    discovery.missing_names()
}

pub fn missing_index_names_with_preferences(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
    preferred_versions: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let mut discovery = IndexDiscovery::new_with_preferences(metadata, source, preferred_versions)?;
    discovery.add_entries(index_files)?;
    discovery.missing_names()
}

/// Which sparse-index files a resolve still needs, kept across fetch waves.
///
/// Edges reaching the same package are unified the way resolution unifies
/// them, and a package is walked again whenever that union grows. Feature
/// activation is not monotone across edges, so a union can reach a crate no
/// single edge reaches.
///
/// Each index file is parsed once, when it is added, and each call to
/// [`IndexDiscovery::missing_names`] walks only the edges that stopped at a
/// package missing on the previous call, so a graph fetched in W waves costs
/// one walk rather than W.
pub struct IndexDiscovery {
    registry: Registry,
    pending: VecDeque<RegistryDependency>,
    discovered: BTreeMap<PackageKey, Discovered>,
    visited: BTreeSet<VisitKey>,
    blocked: BTreeMap<VisitKey, RegistryDependency>,
    preferred_versions: PreferredVersions,
}

type VisitKey = (String, String, bool, BTreeSet<String>);

impl IndexDiscovery {
    pub fn new(metadata: &str, source: &str) -> Result<Self> {
        Self::new_with_parsed_preferences(metadata, source, PreferredVersions::new())
    }

    pub fn new_with_preferences(
        metadata: &str,
        source: &str,
        preferred_versions: &BTreeMap<String, Vec<String>>,
    ) -> Result<Self> {
        Self::new_with_parsed_preferences(
            metadata,
            source,
            parse_preferred_versions(preferred_versions)?,
        )
    }

    fn new_with_parsed_preferences(
        metadata: &str,
        source: &str,
        preferred_versions: PreferredVersions,
    ) -> Result<Self> {
        let metadata = parse_metadata(metadata)?;
        Ok(Self {
            registry: Registry::empty(source),
            pending: VecDeque::from(root_dependencies(&metadata)?),
            discovered: BTreeMap::new(),
            visited: BTreeSet::new(),
            blocked: BTreeMap::new(),
            preferred_versions,
        })
    }

    /// Parse and add index files. An entry replaces one already added under
    /// the same crate name, but edges walked against the replaced entry are
    /// not walked again, so add only names [`IndexDiscovery::missing_names`]
    /// reported.
    pub fn add_entries(&mut self, index_files: &BTreeMap<String, String>) -> Result<()> {
        self.add_entries_until(index_files, &|| false)
    }

    /// [`IndexDiscovery::add_entries`] with cooperative cancellation checked
    /// while parsing index entries.
    pub fn add_entries_until(
        &mut self,
        index_files: &BTreeMap<String, String>,
        should_cancel: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<()> {
        self.registry.add_entries_until(index_files, should_cancel)
    }

    /// Names still missing after walking every edge the added entries unblock.
    pub fn missing_names(&mut self) -> Result<Vec<String>> {
        self.missing_names_until(&|| false)
    }

    /// [`IndexDiscovery::missing_names`] with cooperative cancellation checked
    /// on each dependency and version scan step.
    pub fn missing_names_until(
        &mut self,
        should_cancel: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<String>> {
        self.pending.extend(std::mem::take(&mut self.blocked).into_values());
        while let Some(dependency) = self.pending.pop_front() {
            self.walk_dependency(dependency, should_cancel)?;
        }
        let missing = self.blocked
            .values()
            .map(|dependency| dependency.name.clone())
            .collect::<BTreeSet<_>>();
        Ok(missing.into_iter().collect())
    }

    fn walk_dependency(
        &mut self,
        dependency: RegistryDependency,
        should_cancel: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<()> {
        if should_cancel() {
            self.pending.push_front(dependency);
            return Err(CancelError.into());
        }
        self.registry.validate_dependency_source(dependency.registry.as_deref())?;
        let visit_key = (
            dependency.name.clone(),
            dependency.requirement.to_string(),
            dependency.default_features,
            dependency.features.clone(),
        );
        if self.visited.contains(&visit_key) {
            return Ok(());
        }
        let Some(versions) = self.registry.versions(&dependency.name) else {
            self.blocked.insert(visit_key, dependency);
            return Ok(());
        };
        match unified_dependencies(
            &mut self.discovered,
            &dependency,
            versions,
            &self.preferred_versions,
            should_cancel,
        ) {
            Ok(deps) => {
                self.visited.insert(visit_key);
                self.pending.extend(deps);
                Ok(())
            }
            Err(err) => {
                if err.is::<CancelError>() {
                    self.pending.push_front(dependency);
                }
                Err(err)
            }
        }
    }
}

struct CompatibleLineGroup<'a> {
    compatibility: String,
    newest_matching: Version,
    matching: Vec<&'a RegistryVersion>,
    all_selectable: Vec<&'a RegistryVersion>,
}

#[derive(Default)]
struct PendingLineGroup<'a> {
    newest_matching: Option<Version>,
    matching: Vec<&'a RegistryVersion>,
    all_selectable: Vec<&'a RegistryVersion>,
}

fn matching_line_groups<'a>(
    versions: &'a [RegistryVersion],
    requirement: &semver::VersionReq,
    name: &str,
    preferred_versions: &PreferredVersions,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Vec<CompatibleLineGroup<'a>>> {
    let mut by_line: BTreeMap<String, PendingLineGroup<'a>> = BTreeMap::new();
    for version in versions
        .iter()
        .filter(|version| {
            !version.yanked || is_preferred(preferred_versions, name, &version.version)
        })
    {
        ensure_not_cancelled(should_cancel)?;
        let entry = by_line
            .entry(compatibility_line(&version.version))
            .or_default();
        entry.all_selectable.push(version);
        if requirement.matches(&version.version) {
            entry.newest_matching = Some(version.version.clone());
            entry.matching.push(version);
        }
    }
    let mut groups: Vec<CompatibleLineGroup<'a>> = by_line
        .into_iter()
        .filter_map(|(compatibility, group)| {
            group.newest_matching.map(|newest_matching| CompatibleLineGroup {
                compatibility,
                newest_matching,
                matching: group.matching,
                all_selectable: group.all_selectable,
            })
        })
        .collect();
    groups.sort_by(|left, right| left.newest_matching.cmp(&right.newest_matching));
    Ok(groups)
}

/// Fold what `dependency` asks of its package into what discovery already
/// knows, and return the dependencies of every version that now needs
/// walking: the ones this edge brings into range, or all of them when the
/// unified feature selection grew.
fn unified_dependencies(
    discovered: &mut BTreeMap<PackageKey, Discovered>,
    dependency: &RegistryDependency,
    versions: &[RegistryVersion],
    preferred_versions: &PreferredVersions,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Vec<RegistryDependency>> {
    let mut reached = Vec::new();
    let mut staged = Vec::new();
    let selection = dependency.feature_selection();
    for group in matching_line_groups(
        versions,
        &dependency.requirement,
        &dependency.name,
        preferred_versions,
        should_cancel,
    )? {
        ensure_not_cancelled(should_cancel)?;
        let selectable = selectable_versions(&group.matching, &selection, should_cancel)?;
        let package = PackageKey::Registry {
            name: dependency.name.clone(),
            compatibility: group.compatibility,
        };
        let (entry, deps) = stage_compatibility_line(
            discovered.get(&package),
            dependency,
            &group.all_selectable,
            selectable,
            should_cancel,
        )?;
        staged.push((package, entry));
        reached.extend(deps);
    }
    discovered.extend(staged);
    Ok(reached)
}

fn selectable_versions(
    matching_versions: &[&RegistryVersion],
    selection: &FeatureSelection,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<BTreeSet<Version>> {
    // Only what this dependency could settle on: a version missing a
    // feature it asks for is not one it can select, even though another
    // dependency on the same line may select it.
    let mut selectable = BTreeSet::new();
    for version in matching_versions {
        ensure_not_cancelled(should_cancel)?;
        if supports_features(version, selection) {
            selectable.insert(version.version.clone());
        }
    }
    Ok(selectable)
}

fn stage_compatibility_line(
    existing: Option<&Discovered>,
    dependency: &RegistryDependency,
    line_versions: &[&RegistryVersion],
    selectable: BTreeSet<Version>,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(Discovered, Vec<RegistryDependency>)> {
    let mut merged = existing.map(|entry| entry.selection.clone()).unwrap_or_default();
    merged.default_features |= dependency.default_features;
    merged.features.extend(dependency.features.iter().cloned());
    let selection_unchanged = existing.is_some_and(|entry| entry.selection == merged);
    let unwalked = existing
        .map(|entry| &selectable - &entry.versions)
        .unwrap_or_else(|| selectable.clone());
    let mut all_versions = existing.map(|entry| entry.versions.clone()).unwrap_or_default();
    all_versions.extend(selectable);
    let walk = if selection_unchanged { &unwalked } else { &all_versions };
    // The features of every dependency reaching the line, because one
    // may turn on a weak feature of another's. A version that has none
    // of them activates nothing extra.
    let mut deps = Vec::new();
    for version in line_versions {
        ensure_not_cancelled(should_cancel)?;
        if walk.contains(&version.version) {
            deps.extend(locked_dependencies(version, &merged)?);
        }
    }
    Ok((Discovered { selection: merged, versions: all_versions }, deps))
}
