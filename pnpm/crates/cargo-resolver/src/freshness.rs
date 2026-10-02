use crate::{
    metadata::parse_metadata,
    model::{MetadataDependency, MetadataPackage},
};
use cargo_lock::{Dependency, Lockfile, Package};
use miette::{IntoDiagnostic, Report, Result, WrapErr};
use semver::{Version, VersionReq};
use std::str::FromStr;

/// Check that, for every registry or git dependency a workspace member
/// declares, `lockfile` records an edge from that member to a version the
/// requirement accepts, and that the member has no locked edge that none of
/// its declared dependencies accounts for.
///
/// `metadata` is the output of `cargo metadata --no-deps`. Path dependencies
/// need no edge of their own, and account for a source-less edge with their
/// name whose version their requirement accepts.
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

/// Fail when a locked edge of `locked_member` has no declared dependency of
/// its own. One declaration resolves to a single version, so it accounts for
/// at most one edge, while several declarations may share an edge.
fn verify_no_stale_edge(member: &MetadataPackage, locked_member: &Package) -> Result<()> {
    let mut claims = EdgeClaims {
        declarations: &member.dependencies,
        edges: &locked_member.dependencies,
        claimed_edge: vec![None; member.dependencies.len()],
    };
    let stale = (0..claims.edges.len()).find(|&edge| {
        !claims.claim(edge, &mut vec![false; claims.declarations.len()])
    });
    match stale.map(|edge| &claims.edges[edge]) {
        None => Ok(()),
        Some(edge) => Err(outdated(&format!(
            "Cargo.lock locks {} {} for {}, which no dependency of {} accounts for",
            edge.name, edge.version, member.name, member.name,
        ))),
    }
}

/// An assignment of locked edges to distinct declarations that accept them,
/// grown one edge at a time along augmenting paths.
struct EdgeClaims<'a> {
    declarations: &'a [MetadataDependency],
    edges: &'a [Dependency],
    claimed_edge: Vec<Option<usize>>,
}

impl EdgeClaims<'_> {
    fn claim(&mut self, edge: usize, visited: &mut [bool]) -> bool {
        for declaration in 0..self.declarations.len() {
            if visited[declaration]
                || !accounts_for(&self.declarations[declaration], &self.edges[edge])
            {
                continue;
            }
            visited[declaration] = true;
            let free = match self.claimed_edge[declaration] {
                None => true,
                Some(previous) => self.claim(previous, visited),
            };
            if free {
                self.claimed_edge[declaration] = Some(edge);
                return true;
            }
        }
        false
    }
}

fn accounts_for(declaration: &MetadataDependency, edge: &Dependency) -> bool {
    declaration.name == edge.name.as_str()
        && match declaration.source {
            None => edge.source.is_none() && accepts_version(declaration, &edge.version),
            Some(_) => accepts_version(declaration, &edge.version),
        }
}

/// A path or git dependency declared without a version appears in the
/// metadata as `*`. Cargo accepts any version for it, prereleases included,
/// while semver's `*` rejects prereleases. A registry `*` is a requirement
/// the manifest spells out, so it keeps semver's rule.
fn accepts_version(declaration: &MetadataDependency, version: &Version) -> bool {
    let unversioned = declaration.req == VersionReq::STAR
        && declaration.source
            .as_deref()
            .is_none_or(|source| source.starts_with("git+"));
    unversioned || declaration.req.matches(version)
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
    if locked.iter().any(|version| accepts_version(dependency, version)) {
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
