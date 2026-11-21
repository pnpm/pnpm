use crate::{
    metadata::{
        LocalPackage,
        local_packages,
        parse_metadata,
    },
    model::{
        RegistryDependency,
        accepts_declared_version,
    },
};
use cargo_lock::{
    Dependency,
    Lockfile,
    Package,
    SourceId,
};
use miette::{
    IntoDiagnostic,
    Report,
    Result,
    WrapErr,
};
use std::str::FromStr;

/// Check that `lockfile` locks every [local package](local_packages) with an
/// edge for each dependency Cargo records for it, to a version the
/// requirement accepts, and with no edge that none of those dependencies
/// accounts for.
///
/// `metadata` is the output of `cargo metadata --no-deps`, with the packages
/// of path dependencies outside the workspace appended. A path dependency
/// accounts for a source-less edge. A git dependency does not account for an
/// edge to another reference of the same repository. Other sources are not
/// compared, because a `[patch]` legitimately locks a dependency from a source
/// other than the one its manifest names.
pub fn verify_lockfile(metadata: &str, lockfile: &str) -> Result<()> {
    let metadata = parse_metadata(metadata)?;
    let lockfile = Lockfile::from_str(lockfile).into_diagnostic().wrap_err("parse Cargo.lock")?;
    local_packages(&metadata)?.iter().try_for_each(|local| verify_local_package(local, &lockfile))
}

fn verify_local_package(local: &LocalPackage<'_>, lockfile: &Lockfile) -> Result<()> {
    let LocalPackage { package, member, dependencies } = local;
    let locked_package = lockfile.packages
        .iter()
        .find(|locked| {
            locked.source.is_none()
                && locked.name.as_str() == package.name
                && locked.version == package.version
        })
        .ok_or_else(|| {
            let kind = if *member { "workspace member" } else { "path dependency" };
            outdated(&format!(
                "Cargo.lock does not lock the {kind} {} {}",
                package.name, package.version,
            ))
        })?;
    dependencies
        .iter()
        .try_for_each(|dependency| verify_edge(&package.name, dependency, locked_package))?;
    verify_no_stale_edge(&package.name, dependencies, locked_package)
}

fn verify_no_stale_edge(
    member: &str,
    declarations: &[RegistryDependency],
    locked_member: &Package,
) -> Result<()> {
    let mut claims = EdgeClaims {
        declarations,
        edges: &locked_member.dependencies,
        claimed_edge: vec![None; declarations.len()],
    };
    let stale = (0..claims.edges.len()).find(|&edge| {
        !claims.claim(edge, &mut vec![false; claims.declarations.len()])
    });
    match stale.map(|edge| &claims.edges[edge]) {
        None => Ok(()),
        Some(edge) => Err(outdated(&format!(
            "Cargo.lock locks {} for {member}, which no dependency of {member} accounts for",
            describe_edge(edge),
        ))),
    }
}

/// An assignment of locked edges to distinct declarations that accept them,
/// grown one edge at a time along augmenting paths.
struct EdgeClaims<'a> {
    declarations: &'a [RegistryDependency],
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

fn accounts_for(declaration: &RegistryDependency, edge: &Dependency) -> bool {
    declaration.name == edge.name.as_str()
        && match declaration.registry.as_deref() {
            None => edge.source.is_none(),
            Some(source) => !is_other_git_reference(source, edge.source.as_ref()),
        }
        && accepts_declared_version(
            &declaration.requirement,
            declaration.registry.as_deref(),
            &edge.version,
        )
}

/// Whether `locked` is the git repository `declared` names, checked out at a
/// different branch, tag, or revision. Cargo refuses a `[patch]` that points
/// to the repository it patches, so only a stale lockfile records that.
fn is_other_git_reference(declared: &str, locked: Option<&SourceId>) -> bool {
    let (Ok(declared), Some(locked)) = (SourceId::from_url(declared), locked) else {
        return false;
    };
    declared.is_git()
        && locked.is_git()
        && declared.url() == locked.url()
        && declared.git_reference() != locked.git_reference()
}

fn verify_edge(
    member: &str,
    dependency: &RegistryDependency,
    locked_member: &Package,
) -> Result<()> {
    if locked_member.dependencies.iter().any(|edge| accounts_for(dependency, edge)) {
        return Ok(());
    }
    let locked = locked_member.dependencies
        .iter()
        .filter(|edge| edge.name.as_str() == dependency.name)
        .map(describe_edge)
        .collect::<Vec<_>>();
    let name = &dependency.name;
    let declared = match dependency.registry.as_deref() {
        Some(source) if source.starts_with("git+") => {
            format!("{name} {} from {source}", dependency.requirement)
        }
        _ => format!("{name} {}", dependency.requirement),
    };
    if locked.is_empty() {
        return Err(outdated(&format!(
            "{member} depends on {declared}, but Cargo.lock locks no {name} for {member}",
        )));
    }
    let locked = locked.join(", ");
    Err(outdated(&format!("{member} depends on {declared}, but Cargo.lock locks {locked}")))
}

/// An edge as the messages name it, with its source when it is a git one,
/// since the reference is what tells two locks of one repository apart.
fn describe_edge(edge: &Dependency) -> String {
    match edge.source.as_ref().filter(|source| source.is_git()) {
        Some(source) => format!("{} {} from {source}", edge.name, edge.version),
        None => format!("{} {}", edge.name, edge.version),
    }
}

fn outdated(message: &str) -> Report {
    miette::miette!(
        code = "ERR_PNPM_OUTDATED_LOCKFILE",
        help = "Run `cargo update --workspace` to bring Cargo.lock in line with Cargo.toml.",
        "{message}"
    )
}
