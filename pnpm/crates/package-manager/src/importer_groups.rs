//! Which manifest groups one dependency belongs to in an importer record.
//!
//! Shared by both lockfile writers — the fresh build
//! ([`crate::dependencies_graph_to_lockfile`]) and the importers fast path
//! ([`crate::fast_update_importers`]) — so a lockfile either of them writes
//! describes the manifest the same way.

use pnpm_lockfile::{PkgName, ResolvedDependencyMap, ResolvedDependencySpec};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashMap;

/// The importer groups one dependency is recorded under.
///
/// A manifest may declare a dependency in several groups — a project that
/// runs *and* builds with it lists it in `dependencies` and
/// `devDependencies` — and its importer record belongs under each of them, so
/// a single [`DependencyGroup`] cannot carry the placement (pnpm/pnpm#9572).
///
/// `optionalDependencies` is the one overriding group: a dependency it lists
/// is optional whichever other groups list it, and npm reads that entry as
/// the whole truth about the version, so a `dependencies` entry for it is not
/// recorded. A `devDependencies` entry is kept alongside, because dropping it
/// is what made `--dev` skip a dependency the project builds with.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImporterGroups(u8);

/// The importer groups, in the order a record is placed in them.
const PLACEMENT_ORDER: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

/// The manifest fields, lowest precedence first. Scanners run in this order so
/// the last declaration wins where one alias needs a single specifier:
/// `optionalDependencies` wins over `dependencies` wins over
/// `devDependencies`.
const DECLARATION_ORDER: [DependencyGroup; 3] =
    [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional];

impl ImporterGroups {
    pub(crate) fn insert(&mut self, group: DependencyGroup) {
        self.0 |= 1 << group_bit(group);
    }

    pub(crate) fn remove(&mut self, group: DependencyGroup) {
        self.0 &= !(1 << group_bit(group));
    }

    pub(crate) fn contains(self, group: DependencyGroup) -> bool {
        self.0 & (1 << group_bit(group)) != 0
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = DependencyGroup> {
        PLACEMENT_ORDER
            .into_iter()
            .filter(move |group| self.contains(*group))
    }
}

fn group_bit(group: DependencyGroup) -> u8 {
    match group {
        DependencyGroup::Prod => 0,
        DependencyGroup::Dev => 1,
        DependencyGroup::Optional => 2,
        DependencyGroup::Peer => unreachable!("peerDependencies is not an importer group"),
    }
}

/// Each alias the manifest declares, with the specifier an importer record is
/// compared against and the groups that record belongs under.
///
/// The specifier is the highest-precedence declaration — optional wins over
/// prod, prod over dev, matching what [`ImporterGroups`] keeps.
pub(crate) fn manifest_alias_to_declared(
    manifest: &PackageManifest,
) -> HashMap<String, (&str, ImporterGroups)> {
    let mut declared: HashMap<String, (&str, ImporterGroups)> = HashMap::new();
    for group in DECLARATION_ORDER {
        for (name, specifier) in manifest.dependencies([group]) {
            let entry = declared.entry(name.to_string()).or_default();
            entry.0 = specifier;
            entry.1.insert(group);
        }
    }
    for (_, groups) in declared.values_mut() {
        if groups.contains(DependencyGroup::Optional) {
            groups.remove(DependencyGroup::Prod);
        }
    }
    declared
}

/// Each alias the manifest declares, mapped to the groups its importer record
/// belongs under.
///
/// The fresh writer records an alias in every group it is declared in:
/// `dependencies` and `devDependencies` are independent declarations, so an
/// alias in both is both a prod and a dev dependency of the importer.
/// Collapsing it into one made the lockfile lossy, and a group-filtered
/// install then materialized nothing for it — `pnpm install --dev` skipped an
/// `is-even` that `devDependencies` listed as well (pnpm/pnpm#9572).
pub(crate) fn manifest_alias_to_groups(
    manifest: &PackageManifest,
) -> HashMap<String, ImporterGroups> {
    manifest_alias_to_declared(manifest)
        .into_iter()
        .map(|(alias, (_, groups))| (alias, groups))
        .collect()
}

/// One importer's direct dependencies, split by the manifest group they were
/// declared in.
#[derive(Default)]
pub(crate) struct ImporterDependencyGroups {
    pub(crate) prod: ResolvedDependencyMap,
    pub(crate) dev: ResolvedDependencyMap,
    pub(crate) optional: ResolvedDependencyMap,
}

impl ImporterDependencyGroups {
    fn insert(&mut self, group: DependencyGroup, name: PkgName, spec: ResolvedDependencySpec) {
        match group {
            DependencyGroup::Dev => self.dev.insert(name, spec),
            DependencyGroup::Optional => self.optional.insert(name, spec),
            DependencyGroup::Prod | DependencyGroup::Peer => self.prod.insert(name, spec),
        };
    }

    /// Record one direct dependency under every manifest group that declares
    /// it, so the importer stays a faithful description of the manifest.
    /// `None` means the manifest doesn't declare the alias at all (an
    /// auto-installed peer hoisted into the importer's direct deps), which
    /// keeps the prod placement.
    pub(crate) fn insert_alias(
        &mut self,
        groups: Option<ImporterGroups>,
        name: PkgName,
        spec: ResolvedDependencySpec,
    ) {
        let Some(groups) = groups else {
            self.insert(DependencyGroup::Prod, name, spec);
            return;
        };
        for group in groups.iter() {
            self.insert(group, name.clone(), spec.clone());
        }
    }
}
