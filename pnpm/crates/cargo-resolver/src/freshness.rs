use crate::{
    metadata::parse_metadata,
    model::{MetadataDependency, MetadataPackage},
};
use cargo_lock::{Lockfile, Package};
use miette::{IntoDiagnostic, Report, Result, WrapErr};
use semver::Version;
use std::str::FromStr;

/// Check that, for every registry or git dependency a workspace member
/// declares, `lockfile` records an edge from that member to a version the
/// requirement accepts, and that the member has no locked edge that none of
/// its declared dependencies accounts for.
///
/// `metadata` is the output of `cargo metadata --no-deps`. Path dependencies
/// carry no requirement a lockfile could contradict, so they need no edge of
/// their own and account for any edge with their name.
/// Sources are not compared, because a `[patch]` legitimately locks a
/// dependency from a source other than the one its manifest names.
pub fn verify_lockfile(metadata: &str, lockfile: &str) -> Result<()> {
    let metadata = parse_metadata(metadata)?;
    let lockfile = Lockfile::from_str(lockfile).into_diagnostic().wrap_err("parse Cargo.lock")?;
    metadata.packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .try_for_each(|member| verify_member(member, &lockfile))
}

fn verify_member(member: &MetadataPackage, lockfile: &Lockfile) -> Result<()> {
    let locked_member = lockfile.packages
        .iter()
        .find(|package| {
            package.source.is_none()
                && package.name.as_str() == member.name
                && package.version == member.version
        })
        .ok_or_else(|| {
            outdated(&format!(
                "Cargo.lock does not lock the workspace member {} {}",
                member.name, member.version,
            ))
        })?;
    member.dependencies
        .iter()
        .filter(|dependency| dependency.source.is_some())
        .try_for_each(|dependency| verify_edge(&member.name, dependency, locked_member))?;
    verify_no_stale_edge(member, locked_member)
}

fn verify_no_stale_edge(member: &MetadataPackage, locked_member: &Package) -> Result<()> {
    let stale = locked_member.dependencies
        .iter()
        .find(|edge| {
            !member.dependencies
                .iter()
                .any(|dependency| {
                    dependency.name == edge.name.as_str()
                        && (dependency.source.is_none() || dependency.req.matches(&edge.version))
                })
        });
    match stale {
        None => Ok(()),
        Some(edge) => Err(outdated(&format!(
            "Cargo.lock locks {} {} for {}, which no dependency of {} requires",
            edge.name, edge.version, member.name, member.name,
        ))),
    }
}

fn verify_edge(
    member: &str,
    dependency: &MetadataDependency,
    locked_member: &Package,
) -> Result<()> {
    let locked = locked_member.dependencies
        .iter()
        .filter(|edge| edge.name.as_str() == dependency.name)
        .map(|edge| &edge.version)
        .collect::<Vec<&Version>>();
    if locked
        .iter()
        .any(|version| dependency.req.matches(version))
    {
        return Ok(());
    }
    let (name, requirement) = (&dependency.name, &dependency.req);
    if locked.is_empty() {
        return Err(outdated(&format!(
            "{member} depends on {name} {requirement}, but Cargo.lock locks no {name} for {member}",
        )));
    }
    let versions = locked
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    Err(outdated(&format!(
        "{member} depends on {name} {requirement}, but Cargo.lock locks {name} {versions}",
    )))
}

fn outdated(message: &str) -> Report {
    miette::miette!(
        code = "ERR_PNPM_OUTDATED_LOCKFILE",
        help = "Run `cargo update --workspace` to bring Cargo.lock in line with Cargo.toml.",
        "{message}"
    )
}
