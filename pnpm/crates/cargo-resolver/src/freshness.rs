use crate::metadata::parse_metadata;
use cargo_lock::Lockfile;
use miette::{IntoDiagnostic, Result, WrapErr};
use std::str::FromStr;

/// Check that `lockfile` locks, for every registry or git dependency a
/// workspace member declares, a version its requirement accepts.
///
/// `metadata` is the output of `cargo metadata --no-deps`. Path dependencies
/// carry no requirement a lockfile could contradict, so they are skipped.
pub fn verify_lockfile(metadata: &str, lockfile: &str) -> Result<()> {
    let metadata = parse_metadata(metadata)?;
    let lockfile = Lockfile::from_str(lockfile).into_diagnostic().wrap_err("parse Cargo.lock")?;
    let members = metadata.packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id));
    for member in members {
        let dependencies =
            member.dependencies.iter().filter(|dependency| dependency.source.is_some());
        for dependency in dependencies {
            let locked = lockfile.packages
                .iter()
                .filter(|package| package.name.as_str() == dependency.name)
                .map(|package| &package.version)
                .collect::<Vec<_>>();
            if locked
                .iter()
                .any(|version| dependency.req.matches(version))
            {
                continue;
            }
            let (name, requirement, member) = (&dependency.name, &dependency.req, &member.name);
            return Err(if locked.is_empty() {
                miette::miette!(
                    "{member} depends on {name} {requirement}, which Cargo.lock does not lock"
                )
            } else {
                let versions = locked
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                miette::miette!(
                    "{member} depends on {name} {requirement}, but Cargo.lock locks {name} {versions}"
                )
            });
        }
    }
    Ok(())
}
