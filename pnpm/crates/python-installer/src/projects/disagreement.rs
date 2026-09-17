//! Why the members of a shared environment could not be resolved
//! together, when two of them ask for versions of one distribution that
//! nothing satisfies.
//!
//! pubgrub reports the conflict as a chain of terms, which names the
//! project as one root. The members are what the reader has to change, so
//! the two that disagree are named ahead of that report. The search is
//! bounded by the distinct ranges the members ask for, not by how many
//! members ask them.

use super::Member;
use crate::registry::Resolution;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// The resolution failure, naming the two members that disagree when two
/// of them do.
pub(crate) fn disagreement(
    resolution: &Resolution,
    members: &[Member],
    error: miette::Report,
) -> miette::Report {
    match find(resolution, members) {
        Some(found) => error.wrap_err(found),
        None => error,
    }
}

/// One version range members declare on this environment, and every
/// member that declares it.
struct Asked<'a> {
    requirement: &'a Requirement,
    specifiers: &'a VersionSpecifiers,
    roots: BTreeSet<&'a Path>,
}

impl<'a> Asked<'a> {
    /// A member asking this range and a different member asking `other`,
    /// or `None` when one member alone asks both.
    fn distinct_roots(&self, other: &Self) -> Option<(&'a Path, &'a Path)> {
        self.roots
            .iter()
            .find_map(|root| {
                other.roots
                    .iter()
                    .find(|second| *second != root)
                    .map(|second| (*root, *second))
            })
    }
}

fn find(resolution: &Resolution, members: &[Member]) -> Option<String> {
    let environment = &resolution.target.environment;
    // One entry per distinct range, with every member asking it: two
    // members asking the same range cannot disagree with each other, so
    // the pairs compared are of distinct ranges, each reported for a
    // member of its own.
    let mut by_name = BTreeMap::<&PackageName, BTreeMap<String, Asked<'_>>>::new();
    for member in members {
        for requirement in &member.requirements.all {
            let Some(VersionOrUrl::VersionSpecifier(specifiers)) = &requirement.version_or_url
            else {
                continue;
            };
            if !requirement.marker.evaluate(environment, &[]) {
                continue;
            }
            by_name
                .entry(&requirement.name)
                .or_default()
                .entry(specifiers.to_string())
                .or_insert_with(|| Asked { requirement, specifiers, roots: BTreeSet::new() })
                .roots
                .insert(&member.root);
        }
    }
    by_name
        .into_iter()
        .find_map(|(name, asked)| {
            let offered = resolution.packages.candidates
                .get(name)
                .filter(|offered| !offered.is_empty())?;
            let asked = asked.into_values().collect::<Vec<_>>();
            conflicting_pair(name, &asked, &offered.keys().collect::<Vec<_>>())
        })
}

/// Two members whose requirements on `name` no offered version satisfies
/// at once.
fn conflicting_pair(
    name: &PackageName,
    asked: &[Asked<'_>],
    offered: &[&Version],
) -> Option<String> {
    asked
        .iter()
        .enumerate()
        .find_map(|(position, first)| {
            asked[position + 1..]
                .iter()
                .filter_map(|second| Some((second, first.distinct_roots(second)?)))
                .find(|(second, _)| {
                    !offered
                        .iter()
                        .any(|version| {
                            first.specifiers.contains(version)
                                && second.specifiers.contains(version)
                        })
                })
                .map(|(second, (root, other))| {
                    format!(
                        "the Python projects sharing one environment cannot be installed together: \
                         {} requires `{}` and {} requires `{}`, and no version of {name} the index \
                         offers satisfies both",
                        root.display(),
                        first.requirement,
                        other.display(),
                        second.requirement,
                    )
                })
        })
}
