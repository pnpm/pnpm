//! What a resolution failure says about a distribution it had no version
//! of.
//!
//! pubgrub reports a requirement it could satisfy with nothing as an
//! empty version set, spelled `∅`. That is the same answer whether the
//! index has no such distribution, whether it has one but publishes
//! nothing this machine can install, or whether the project asked for a
//! version nobody released. The lines added here say which it was.

use super::Package;
use crate::{Packages, packages::NAMED_RELEASES};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pubgrub::{DerivationTree, External, Ranges};
use std::{collections::BTreeSet, fmt::Write as _};

/// What a failed resolution adds to pubgrub's report: one line per
/// distribution it was left with no version of.
pub(super) fn unoffered_distributions(
    tree: &DerivationTree<Package, Ranges<Version>, String>,
    packages: &Packages,
) -> String {
    let mut unoffered = BTreeSet::new();
    collect_unoffered(tree, &mut unoffered);
    let mut explained = String::new();
    for name in unoffered {
        if let Some(line) = describe(name, packages) {
            write!(explained, "\n{line}").expect("writing to a String cannot fail");
        }
    }
    explained
}

/// The distributions the report names with an empty version set.
fn collect_unoffered<'a>(
    tree: &'a DerivationTree<Package, Ranges<Version>, String>,
    unoffered: &mut BTreeSet<&'a PackageName>,
) {
    match tree {
        DerivationTree::External(external) => match external {
            External::NoVersions(package, range) | External::Custom(package, range, _) => {
                insert_unoffered(package, range, unoffered);
            }
            External::FromDependencyOf(dependent, dependent_range, dependency, range) => {
                insert_unoffered(dependent, dependent_range, unoffered);
                insert_unoffered(dependency, range, unoffered);
            }
            External::NotRoot(_, _) => {}
        },
        DerivationTree::Derived(derived) => {
            collect_unoffered(&derived.cause1, unoffered);
            collect_unoffered(&derived.cause2, unoffered);
        }
    }
}

fn insert_unoffered<'a>(
    package: &'a Package,
    range: &Ranges<Version>,
    unoffered: &mut BTreeSet<&'a PackageName>,
) {
    if let Package::Distribution(name, _) = package
        && range.is_empty()
    {
        unoffered.insert(name);
    }
}

/// Why a distribution had no version to offer, or `None` when nothing
/// this resolution read says anything about it.
fn describe(name: &PackageName, packages: &Packages) -> Option<String> {
    if let Some(offered) = packages.candidates
        .get(name)
        .filter(|offered| !offered.is_empty())
    {
        return Some(format!(
            "{name} is offered at {}, and this project's requirements select none of them.",
            named_releases(offered.keys().rev(), offered.len()),
        ));
    }
    let excluded = packages.excluded.get(name)?;
    if !excluded.published {
        return Some(format!("No index pnpm reads publishes a distribution named {name}."));
    }
    let releases = excluded.releases();
    if releases == 0 {
        return Some(format!(
            "No file the index publishes for {name} is a wheel or a source distribution.",
        ));
    }
    let mut described = format!(
        "{name} publishes {releases} releases ({}), none of which publishes a wheel this \
         interpreter installs or a source distribution pnpm can build.",
        named_releases(
            excluded.other_targets.newest().chain(excluded.other_interpreters.newest()),
            releases,
        ),
    );
    if !excluded.other_interpreters.is_empty() {
        write!(
            described,
            " {} of them declare a Requires-Python this interpreter is outside of.",
            excluded.other_interpreters.len(),
        )
        .expect("writing to a String cannot fail");
    }
    Some(described)
}

/// The newest releases of `total`, named as far as a line names them.
fn named_releases<'a>(newest: impl IntoIterator<Item = &'a Version>, total: usize) -> String {
    let mut named = newest.into_iter().collect::<Vec<_>>();
    named.sort_unstable();
    named.reverse();
    named.truncate(NAMED_RELEASES);
    let listed = named
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    match total.saturating_sub(named.len()) {
        0 => listed,
        rest => format!("{listed} and {rest} older"),
    }
}
