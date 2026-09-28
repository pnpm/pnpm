use crate::{metadata::parse_metadata, model::MetadataDependency};
use cargo_lock::Lockfile;
use miette::{IntoDiagnostic, Report, Result, WrapErr};
use semver::Version;
use std::{collections::BTreeMap, str::FromStr};

/// Check that `lockfile` locks, for every registry or git dependency a
/// workspace member declares, a version its requirement accepts.
///
/// `metadata` is the output of `cargo metadata --no-deps`. Path dependencies
/// carry no requirement a lockfile could contradict, so they are skipped.
pub fn verify_lockfile(metadata: &str, lockfile: &str) -> Result<()> {
    let metadata = parse_metadata(metadata)?;
    let lockfile = Lockfile::from_str(lockfile).into_diagnostic().wrap_err("parse Cargo.lock")?;
    let locked_versions = locked_versions_by_name(&lockfile);
    metadata.packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .flat_map(|member| {
            member.dependencies
                .iter()
                .filter(|dependency| dependency.source.is_some())
                .map(move |dependency| (member.name.as_str(), dependency))
        })
        .try_for_each(|(member, dependency)| {
            let locked = locked_versions
                .get(dependency.name.as_str())
                .map_or(&[][..], Vec::as_slice);
            if locked
                .iter()
                .any(|version| dependency.req.matches(version))
            {
                Ok(())
            } else {
                Err(unsatisfied(member, dependency, locked))
            }
        })
}

fn locked_versions_by_name(lockfile: &Lockfile) -> BTreeMap<&str, Vec<&Version>> {
    let mut locked_versions = BTreeMap::<&str, Vec<&Version>>::new();
    for package in &lockfile.packages {
        locked_versions
            .entry(package.name.as_str())
            .or_default()
            .push(&package.version);
    }
    locked_versions
}

fn unsatisfied(member: &str, dependency: &MetadataDependency, locked: &[&Version]) -> Report {
    let (name, requirement) = (&dependency.name, &dependency.req);
    if locked.is_empty() {
        return miette::miette!(
            "{member} depends on {name} {requirement}, which Cargo.lock does not lock"
        );
    }
    let versions = locked
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    miette::miette!(
        "{member} depends on {name} {requirement}, but Cargo.lock locks {name} {versions}"
    )
}
