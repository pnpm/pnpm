//! Where an installed package comes from: the projects that declare it, or
//! a chain of packages that brings it in.

use super::{
    AuditGraph,
    EnvLockfile,
    HashMap,
    HashSet,
    Include,
    Lockfile,
    PackageKey,
    build_audit_path_index,
    importer_roots,
    package_version,
    root_included,
    walk_reachable,
};
use crate::cli_args::sanitize::sanitize_inline;
use serde::Serialize;
use std::collections::BTreeSet;

/// How many projects a package's line names before it counts the rest.
const MAX_LISTED_PROJECTS: usize = 3;

/// The longest chain a line names in full. A longer one keeps the
/// dependency a project declares and the [`LISTED_CHAIN_TAIL`] packages
/// closest to the package, with `...` between.
const MAX_LISTED_CHAIN: usize = 4;

const LISTED_CHAIN_TAIL: usize = 2;

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Origin {
    /// The projects that declare the package as a direct dependency.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) projects: Vec<String>,
    /// A chain of packages that brings it in, for a package no project
    /// declares.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) via: Vec<String>,
}

impl Origin {
    /// The chain or the projects, read from the lockfile, which a
    /// contributor can edit, so with control characters removed.
    pub(super) fn render(&self) -> String {
        let clean = |label: &String| sanitize_inline(label).into_owned();
        if !self.via.is_empty() {
            let mut chain: Vec<String> = self.via.iter().map(clean).collect();
            if chain.len() > MAX_LISTED_CHAIN {
                chain.splice(1..chain.len() - LISTED_CHAIN_TAIL, ["...".to_string()]);
            }
            return format!("via {}", chain.join(" > "));
        }
        let listed = self.projects
            .iter()
            .take(MAX_LISTED_PROJECTS)
            .map(clean)
            .collect::<Vec<_>>();
        let rest = self.projects.len() - listed.len();
        if rest == 0 { listed.join(", ") } else { format!("{} and {rest} more", listed.join(", ")) }
    }
}

/// A package by its peer-stripped lockfile key and its version.
pub(super) struct AuditedPackage<'a> {
    pub(super) key: &'a PackageKey,
    pub(super) name: &'a str,
    pub(super) version: &'a str,
}

/// A package an audit reports, which [`Origins::add_to`] fills in the origin
/// of.
pub(super) trait Audited {
    fn audited(&self) -> AuditedPackage<'_>;
    fn origin_mut(&mut self) -> &mut Origin;
}

/// Orders `packages` by name, then version, then lockfile key.
pub(super) fn sort_audited(packages: &mut [impl Audited]) {
    packages.sort_by(|left, right| {
        let (left, right) = (left.audited(), right.audited());
        (left.name, left.version)
            .cmp(&(right.name, right.version))
            .then_with(|| {
                left.key
                    .to_string()
                    .cmp(&right.key.to_string())
            })
    });
}

/// The installed packages of the audited lockfiles, which [`Self::find`]
/// tells the origins of.
pub(super) struct Origins<'a> {
    lockfile: &'a Lockfile,
    env_lockfile: Option<&'a EnvLockfile>,
    include: Include,
    /// The `(name, version)` pairs more than one installed package has,
    /// such as one version installed from two registries.
    shared_versions: HashSet<(String, String)>,
}

impl<'a> Origins<'a> {
    pub(super) fn new(
        lockfile: &'a Lockfile,
        env_lockfile: Option<&'a EnvLockfile>,
        include: Include,
    ) -> Self {
        let mut origins =
            Origins { lockfile, env_lockfile, include, shared_versions: HashSet::new() };
        origins.shared_versions = shared_versions(
            origins
                .graphs()
                .iter()
                .flat_map(|graph| {
                    walk_reachable(graph, include, include.optional_dependencies)
                        .into_iter()
                        .map(|key| key.without_peer())
                        .filter_map(|key| {
                            let version = package_version(&key, Some(graph.packages))?;
                            Some((key, version))
                        })
                        .collect::<Vec<_>>()
                }),
        );
        origins
    }

    /// Sets the origin of each of `packages`.
    pub(super) fn add_to(&self, packages: &mut [impl Audited]) {
        let audited: Vec<_> = packages
            .iter()
            .map(Audited::audited)
            .collect();
        let origins = self.find(&audited);
        for (package, origin) in packages.iter_mut().zip(origins) {
            *package.origin_mut() = origin;
        }
    }

    /// The origin of each of `packages`, in their order. A package that
    /// no project declares gets the first chain that brings it in. The
    /// chains tell packages apart by name and version only, so a shared
    /// version gets no chain rather than another package's.
    fn find(&self, packages: &[AuditedPackage<'_>]) -> Vec<Origin> {
        let declared = self.declarations();
        let names: HashSet<String> = packages
            .iter()
            .map(|package| package.name.to_string())
            .collect();
        let index = build_audit_path_index(self.lockfile, self.env_lockfile, &names, self.include);
        packages
            .iter()
            .map(|package| {
                let projects: Vec<String> = declared
                    .get(package.key)
                    .map(|projects| projects.iter().cloned().collect())
                    .unwrap_or_default();
                let shared = self.shared_versions.contains(&(
                    package.name.to_string(),
                    package.version.to_string(),
                ));
                let via = if projects.is_empty() && !shared {
                    first_chain(&index, package)
                } else {
                    Vec::new()
                };
                Origin { projects, via }
            })
            .collect()
    }

    fn graphs(&self) -> Vec<AuditGraph<'a>> {
        let mut graphs = vec![AuditGraph::main(self.lockfile, self.include.peer_edges)];
        graphs.extend(self.env_lockfile.map(AuditGraph::env));
        graphs
    }

    /// The projects, by their lockfile importer ID, that declare each
    /// package directly, by its peer-stripped key.
    fn declarations(&self) -> HashMap<PackageKey, BTreeSet<String>> {
        let mut declared: HashMap<PackageKey, BTreeSet<String>> = HashMap::new();
        let main_roots = self.lockfile.importers
            .iter()
            .flat_map(|(project, snapshot)| {
                importer_roots(snapshot)
                    .into_iter()
                    .filter(|(kind, _)| root_included(*kind, self.include))
                    .map(move |(_, edge)| (project.clone(), edge.key))
            });
        let env = self.env_lockfile.map(AuditGraph::env);
        let env_roots = env
            .iter()
            .flat_map(|env| &env.importers)
            .flat_map(|importer| {
                importer.roots
                    .iter()
                    .filter(|(kind, _)| root_included(*kind, self.include))
                    .map(move |(_, edge)| (importer.path_segment.clone(), edge.key.clone()))
            });
        for (project, key) in main_roots.chain(env_roots) {
            declared
                .entry(key.without_peer())
                .or_default()
                .insert(project);
        }
        declared
    }
}

/// The packages between the project and `package` on its first path that
/// passes through another package. A path reads `<project>>...>name`.
fn first_chain(index: &super::AuditPathIndex, package: &AuditedPackage<'_>) -> Vec<String> {
    index
        .get(package.name)
        .and_then(|versions| versions.get(package.version))
        .and_then(|info| {
            info.paths
                .iter()
                .map(|path| path.split('>').collect::<Vec<_>>())
                .find(|segments| segments.len() > 2)
        })
        .map(|segments| {
            segments[1..segments.len() - 1]
                .iter()
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `(name, version)` pairs of more than one of `installed`.
fn shared_versions(
    installed: impl Iterator<Item = (PackageKey, String)>,
) -> HashSet<(String, String)> {
    let mut keys: HashMap<(String, String), HashSet<PackageKey>> = HashMap::new();
    for (key, version) in installed {
        keys.entry((key.name.to_string(), version))
            .or_default()
            .insert(key);
    }
    keys.into_iter()
        .filter(|(_, keys)| keys.len() > 1)
        .map(|(version, _)| version)
        .collect()
}

#[cfg(test)]
mod tests;
