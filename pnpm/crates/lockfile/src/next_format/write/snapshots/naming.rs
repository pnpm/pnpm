//! Bounded peer descriptions that distinguish snapshots of the same package.

use super::{
    super::super::{NamedBindings, NextFormatError, NextRef, tarjan, without_groups},
    Snapshot, Snapshots,
};
use crate::{PackageKey, PkgName, SnapshotDepRef};
use std::collections::{HashMap, HashSet};

impl Snapshots<'_> {
    pub(super) fn naming_bindings_in(
        &self,
        target: &PackageKey,
        budget: &mut NamingBudget,
    ) -> Result<Option<NamedBindings>, NextFormatError> {
        let Some(&(package, index)) = self.index.get(target) else { return Ok(None) };
        if self.varying[package].names.is_empty() || budget.visiting.contains(target) {
            return Ok(None);
        }
        budget.enter(target)?;
        let snapshot = &self.by_package[package].1[index];
        let varying = &self.varying[package].names;
        let peers: NamedBindings = snapshot
            .bindings()
            .filter(|(name, _)| varying.contains(*name))
            .map(|(name, reference)| {
                let version = without_groups(reference);
                budget.take_binding()?;
                let nested = self
                    .binding_needs_nested(target, name, reference)
                    .then(|| reference.resolve(name))
                    .flatten()
                    .map(|target| self.naming_bindings_in(&target, budget))
                    .transpose()?
                    .flatten();
                let reference = match nested {
                    Some(peers) => NextRef::Named { version, peers },
                    None => NextRef::Plain(version),
                };
                Ok((name.to_string(), reference))
            })
            .collect::<Result<_, NextFormatError>>()?;
        budget.visiting.remove(target);
        Ok(Some(peers))
    }

    fn binding_needs_nested(
        &self,
        source: &PackageKey,
        name: &PkgName,
        reference: &SnapshotDepRef,
    ) -> bool {
        let Some(target) = reference.resolve(name) else { return false };
        // Cycle edges have one version-only description, independent of the entry point.
        if self.components.get(source) == self.components.get(&target) {
            return false;
        }
        let (package, _) = self.index[source];
        self.varying[package].nested.contains(&(name.clone(), without_groups(reference)))
    }
}

/// The peers that `snapshots`, of one package, bind differently or not all
/// bind.
pub(super) fn varying_bindings(snapshots: &[Snapshot<'_>]) -> BindingVariation {
    let mut values: HashMap<&PkgName, (HashSet<SnapshotDepRef>, usize)> = HashMap::new();
    for snapshot in snapshots {
        for (name, reference) in snapshot.bindings() {
            let (bound, count) = values.entry(name).or_default();
            bound.insert(reference.clone());
            *count += 1;
        }
    }
    let mut variation = BindingVariation::default();
    for (name, (bound, count)) in values {
        if bound.len() > 1 || count < snapshots.len() {
            variation.names.insert(name.clone());
        }
        variation.nested.extend(ambiguous_versions(bound).map(|version| (name.clone(), version)));
    }
    variation
}

#[derive(Default)]
pub(super) struct BindingVariation {
    pub(super) names: HashSet<PkgName>,
    nested: HashSet<(PkgName, SnapshotDepRef)>,
}

pub(super) struct NamingBudget {
    visiting: HashSet<PackageKey>,
    remaining: usize,
}

impl Default for NamingBudget {
    fn default() -> Self {
        Self { visiting: HashSet::new(), remaining: 4096 }
    }
}

impl NamingBudget {
    fn enter(&mut self, target: &PackageKey) -> Result<(), NextFormatError> {
        // Each level adds a reference and a `with` map; leave room for the document's containers.
        if self.visiting.len() >= 32 {
            return Err(NextFormatError::NotRepresentable {
                detail: "peer bindings exceed 32 nested references".to_string(),
            });
        }
        self.visiting.insert(target.clone());
        Ok(())
    }

    fn take_binding(&mut self) -> Result<(), NextFormatError> {
        self.remaining = self.remaining
            .checked_sub(1)
            .ok_or_else(|| NextFormatError::NotRepresentable {
                detail: "a peer reference expands to more than 4096 bindings".to_string(),
            })?;
        Ok(())
    }
}

pub(super) fn binding_components<'a>(
    groups: &[(PackageKey, Vec<Snapshot<'a>>)],
) -> HashMap<&'a PackageKey, usize> {
    let snapshots: Vec<_> = groups
        .iter()
        .flat_map(|(_, snapshots)| snapshots)
        .collect();
    let indices: HashMap<_, _> = snapshots
        .iter()
        .enumerate()
        .map(|(index, snapshot)| (snapshot.key, index))
        .collect();
    tarjan::components(
        snapshots.len(),
        |index| {
            snapshots[index]
                .bindings()
                .filter_map(|(name, reference)| {
                    reference
                        .resolve(name)
                        .and_then(|key| indices.get(&key).copied())
                })
                .collect()
        },
        |_| true,
    )
    .component_of
    .into_iter()
    .map(|(index, component)| (snapshots[index].key, component))
    .collect()
}

fn ambiguous_versions(references: HashSet<SnapshotDepRef>) -> impl Iterator<Item = SnapshotDepRef> {
    let mut seen = HashSet::new();
    references
        .into_iter()
        .map(|reference| without_groups(&reference))
        .filter(move |version| !seen.insert(version.clone()))
}
