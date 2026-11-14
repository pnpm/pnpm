use super::{LockedPackage, Lockfile, marker_tree};
use miette::{IntoDiagnostic, Result, WrapErr};
use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName, Requirement, VersionOrUrl};
use std::collections::BTreeMap;

impl Lockfile {
    /// Build a reusable lookup index over this lockfile's environments and
    /// packages.
    pub fn index(&self) -> Result<LockIndex<'_>> {
        let scope = self.environment_scope()?;
        let mut by_name: BTreeMap<&PackageName, Vec<IndexedPackage<'_>>> = BTreeMap::new();
        for package in &self.packages {
            by_name
                .entry(&package.name)
                .or_default()
                .push(IndexedPackage::from_locked(package, &scope)?);
        }
        Ok(LockIndex { scope, by_name })
    }

    /// The single version this lockfile resolves `requirement` to across every
    /// overlapping environment, or `None` when no environment selects it or
    /// overlapping environments resolve different versions.
    pub fn agreed_version(&self, requirement: &Requirement) -> Result<Option<&Version>> {
        Ok(self.index()?.agreed_version(requirement))
    }

    fn environment_scope(&self) -> Result<MarkerTree> {
        if self.environments.is_empty() {
            return Ok(MarkerTree::TRUE);
        }
        let mut scope = MarkerTree::FALSE;
        for environment in &self.environments {
            let parsed: MarkerTree = environment
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("read Python lockfile environment {environment}"))?;
            scope.or(parsed);
        }
        Ok(scope)
    }
}

/// One locked version of a package and the environment marker under which it
/// applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPin<'a> {
    pub version: &'a Version,
    pub marker: MarkerTree,
}

struct IndexedPackage<'a> {
    version: &'a Version,
    active_marker: MarkerTree,
    package_marker: MarkerTree,
    is_index: bool,
}

impl<'a> IndexedPackage<'a> {
    fn from_locked(package: &'a LockedPackage, scope: &MarkerTree) -> Result<Self> {
        let package_marker = marker_tree(package)?;
        let mut active_marker = scope.clone();
        active_marker.and(package_marker.clone());
        let is_index = !package.wheels.is_empty() || package.sdist.is_some();
        Ok(Self { version: &package.version, active_marker, package_marker, is_index })
    }
}

/// Pre-parsed marker and package lookup index over a [`Lockfile`].
pub struct LockIndex<'a> {
    scope: MarkerTree,
    by_name: BTreeMap<&'a PackageName, Vec<IndexedPackage<'a>>>,
}

impl<'a> LockIndex<'a> {
    /// The single version all overlapping locked entries agree on for
    /// `requirement`, including directory and VCS packages.
    #[must_use]
    pub fn agreed_version(&self, requirement: &Requirement) -> Option<&'a Version> {
        agreed_entry_version(self.matching_entries(requirement))
    }

    /// The single version all overlapping index-backed entries agree on for
    /// `requirement`.
    #[must_use]
    pub fn agreed_index_version(&self, requirement: &Requirement) -> Option<&'a Version> {
        agreed_entry_version(self.matching_entries(requirement).filter(|entry| entry.is_index))
    }

    /// Every overlapping index-backed entry for `requirement`, paired with the
    /// narrowed marker under which that version was locked.
    #[must_use]
    pub fn matching_index_pins(&self, requirement: &Requirement) -> Vec<LockedPin<'a>> {
        self.matching_entries(requirement)
            .filter(|entry| entry.is_index)
            .map(|entry| {
                let mut marker = requirement.marker.clone();
                marker.and(entry.package_marker.clone());
                LockedPin { version: entry.version, marker }
            })
            .collect()
    }

    /// Rewrite `requirement` to pin every environment already covered by an
    /// index-backed entry in this lockfile to its locked version, while leaving
    /// any uncovered marker branch at `requirement`'s original range.
    pub fn pinned_index_requirements(
        &self,
        requirement: &Requirement,
    ) -> Result<Option<Vec<Requirement>>> {
        let pins = self.matching_index_pins(requirement);
        let Some(first) = pins.first() else {
            return Ok(None);
        };
        let uncovered = uncovered_marker(&requirement.marker, &pins);
        if pins
            .iter()
            .all(|pin| pin.version == first.version)
            && self.scope_excludes(&uncovered)
        {
            let pinned =
                with_exact_version(requirement, first.version, requirement.marker.clone())?;
            return Ok(Some(vec![pinned]));
        }
        build_split_pins(requirement, pins, uncovered)
    }

    fn scope_excludes(&self, uncovered: &MarkerTree) -> bool {
        let without_extras = uncovered.clone().simplify_extras_with(|_| true);
        self.scope.is_disjoint(&without_extras)
    }

    fn matching_entries<'b>(
        &'b self,
        requirement: &Requirement,
    ) -> impl Iterator<Item = &'b IndexedPackage<'a>> {
        let wanted = requirement.marker.clone().simplify_extras_with(|_| true);
        self.by_name
            .get(&requirement.name)
            .into_iter()
            .flatten()
            .filter(move |entry| !entry.active_marker.is_disjoint(&wanted))
    }
}

fn uncovered_marker(base: &MarkerTree, pins: &[LockedPin<'_>]) -> MarkerTree {
    let mut covered = MarkerTree::FALSE;
    for pin in pins {
        covered.or(pin.marker.clone());
    }
    let mut uncovered = base.clone();
    uncovered.and(covered.negate());
    uncovered
}

fn build_split_pins(
    requirement: &Requirement,
    pins: Vec<LockedPin<'_>>,
    uncovered: MarkerTree,
) -> Result<Option<Vec<Requirement>>> {
    let mut staged = Vec::with_capacity(pins.len() + 1);
    for pin in pins {
        if !pin.marker.is_false() {
            staged.push(with_exact_version(requirement, pin.version, pin.marker)?);
        }
    }
    if !uncovered.is_false() {
        let mut fallback = requirement.clone();
        fallback.marker = uncovered;
        staged.push(fallback);
    }
    Ok((!staged.is_empty()).then_some(staged))
}

fn with_exact_version(
    requirement: &Requirement,
    version: &Version,
    marker: MarkerTree,
) -> Result<Requirement> {
    let mut pinned = requirement.clone();
    pinned.version_or_url =
        Some(VersionOrUrl::VersionSpecifier(format!("=={version}").parse().into_diagnostic()?));
    pinned.marker = marker;
    Ok(pinned)
}

fn agreed_entry_version<'b, 'a: 'b>(
    mut entries: impl Iterator<Item = &'b IndexedPackage<'a>>,
) -> Option<&'a Version> {
    let first = entries.next()?.version;
    entries
        .all(|entry| entry.version == first)
        .then_some(first)
}
