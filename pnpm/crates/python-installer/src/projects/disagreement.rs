//! Why the members of a shared environment could not be resolved
//! together, when two of them ask for versions of one distribution that
//! nothing satisfies.
//!
//! pubgrub reports the conflict as a chain of terms, which names the
//! project as one root. The members are what the reader has to change, so
//! the two that disagree are named ahead of that report.

use super::Member;
use crate::registry::Resolution;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use std::{collections::BTreeMap, path::Path};

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

/// A version requirement one member declares on this environment.
struct Asked<'a> {
    root: &'a Path,
    requirement: &'a Requirement,
    specifiers: &'a VersionSpecifiers,
}

fn find(resolution: &Resolution, members: &[Member]) -> Option<String> {
    let environment = &resolution.target.environment;
    let mut by_name = BTreeMap::<&PackageName, Vec<Asked<'_>>>::new();
    for member in members {
        for requirement in &member.requirements.all {
            let Some(VersionOrUrl::VersionSpecifier(specifiers)) = &requirement.version_or_url
            else {
                continue;
            };
            if requirement.marker.evaluate(environment, &[]) {
                by_name
                    .entry(&requirement.name)
                    .or_default()
                    .push(Asked { root: &member.root, requirement, specifiers });
            }
        }
    }
    by_name
        .into_iter()
        .find_map(|(name, asked)| {
            let offered = resolution.packages.candidates
                .get(name)
                .filter(|offered| !offered.is_empty())?;
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
                .filter(|second| second.root != first.root)
                .find(|second| {
                    !offered
                        .iter()
                        .any(|version| {
                            first.specifiers.contains(version)
                                && second.specifiers.contains(version)
                        })
                })
                .map(|second| {
                    format!(
                        "the Python projects sharing one environment cannot be installed together: \
                         {} requires `{}` and {} requires `{}`, and no version of {name} the index \
                         offers satisfies both",
                        first.root.display(),
                        first.requirement,
                        second.root.display(),
                        second.requirement,
                    )
                })
        })
}
