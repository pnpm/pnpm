//! Peer-dependency issue inspection, shared by every command that
//! reports unmet peers.
//!
//! The verdict is read off a lockfile rather than off a live
//! resolution: a lockfile records each snapshot's `peerDependencies`
//! next to the dependency refs the resolver settled on, which is all a
//! verdict needs. `pnpm peers check` reads one off disk; an install
//! passes the one it just resolved. Sharing the source is what keeps
//! the two commands from disagreeing about the same tree.
//!
//! Mirrors pnpm's `@pnpm/deps.inspection.peers-checker` (the walk and
//! the `peerDependencyRules` filter) and
//! `@pnpm/deps.inspection.peers-issues-renderer` (the terminal
//! rendering). pnpm's own install derives its issues from the
//! resolution instead, which is the one place the two stacks compute
//! the same verdict by different routes.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;

use pnpm_config::PeerDependencyRules;
use pnpm_lockfile::{Lockfile, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotEntry};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::get_peer_version_range;
use pnpm_text_sanitize::sanitize;

#[derive(Debug, Default, Clone, Serialize)]
pub struct ParentPkg {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BadPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
    #[serde(rename = "foundVersion")]
    pub found_version: String,
    #[serde(rename = "resolvedFrom")]
    pub resolved_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerIssues {
    pub bad: BTreeMap<String, Vec<BadPeerIssue>>,
    pub missing: BTreeMap<String, Vec<MissingPeerIssue>>,
    pub conflicts: Vec<String>,
    pub intersections: BTreeMap<String, String>,
}

pub type IssuesByProjects = BTreeMap<String, PeerIssues>;

/// The issues an install or `pnpm dedupe` acts on: everything left
/// after `peerDependencyRules`, when any of it is worth acting on. A
/// missing peer every parent marks optional is not — pnpm's
/// install-time gate ignores it, and so does `pnpm peers check`.
pub struct PeerIssuesReport {
    issues: IssuesByProjects,
    /// Whether any non-optional peer is absent outright, as opposed to
    /// present at an unsatisfying version. Only the absent case is
    /// answerable by `autoInstallPeers`, so only it earns that hint.
    has_missing_peer: bool,
}

impl PeerIssuesReport {
    /// The listing `pnpm peers check` prints for the same issues. Empty
    /// when every issue is one that listing leaves out — a missing peer no
    /// parent conflicts over, say — so callers must handle an empty body.
    #[must_use]
    pub fn render(&self) -> String {
        render_peer_issues(&self.issues)
    }

    /// The `issuesByProjects` payload of pnpm's
    /// `pnpm:peer-dependency-issues` log.
    #[must_use]
    pub fn issues(&self) -> &IssuesByProjects {
        &self.issues
    }

    /// pnpm's `ERR_PNPM_PEER_DEP_ISSUES` body: the listing, then the
    /// ways out of the failure — auto-installing the peers first when
    /// any is absent, then switching the guard off.
    #[must_use]
    pub fn render_error(&self) -> String {
        let mut hints = Vec::new();
        if self.has_missing_peer {
            hints.push(
                "hint: To auto-install peer dependencies, add the following to \"pnpm-workspace.yaml\" in your project root:\n\n  autoInstallPeers: true",
            );
        }
        hints.push(
            "hint: To disable failing on peer dependency issues, add the following to pnpm-workspace.yaml in your project root:\n\n  strictPeerDependencies: false",
        );
        let hints = hints.join("\n");
        let rendered = self.render();
        let body = if rendered.is_empty() { hints } else { format!("{rendered}\n{hints}") };
        format!("[ERR_PNPM_PEER_DEP_ISSUES] Unmet peer dependencies\n\n{body}\n")
    }
}

/// The same issue set `pnpm peers check` reports, filtered by
/// `peerDependencyRules`, for the given importers.
///
/// Returns `None` when there is nothing worth acting on.
#[must_use]
pub fn peer_issues_for_lockfile(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    importer_ids: &[String],
    rules: &PeerDependencyRules,
) -> Option<PeerIssuesReport> {
    let issues = filter_peer_issues(
        check_peer_dependencies_of_importers(lockfile, lockfile_dir, importer_ids),
        rules,
    );
    let has_missing_peer = issues.values().any(|project_issues| {
        project_issues.missing.values().any(|entries| entries.iter().any(|entry| !entry.optional))
    });
    let has_issues =
        has_missing_peer || issues.values().any(|project_issues| !project_issues.bad.is_empty());
    has_issues.then_some(PeerIssuesReport { issues, has_missing_peer })
}

/// The issues reachable from the given project directories, for the
/// commands that inspect a selection rather than the whole workspace.
#[must_use]
pub fn check_peer_dependencies_from_lockfile(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
) -> IssuesByProjects {
    let mut importer_ids: Vec<String> = project_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir))
        .filter(|importer_id| lockfile.importers.contains_key(importer_id))
        .collect();
    importer_ids.sort();
    importer_ids.dedup();
    check_peer_dependencies_of_importers(lockfile, lockfile_dir, &importer_ids)
}

/// Walk the named importers, collecting every peer a package requires
/// but the recorded graph does not satisfy. Unfiltered — the caller
/// applies [`filter_peer_issues`].
#[must_use]
pub fn check_peer_dependencies_of_importers(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    importer_ids: &[String],
) -> IssuesByProjects {
    let empty_packages = HashMap::new();
    let empty_snapshots = HashMap::new();
    let packages = lockfile.packages.as_ref().unwrap_or(&empty_packages);
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty_snapshots);

    let mut result: IssuesByProjects = BTreeMap::new();
    // Shared across importers so each package is evaluated once, matching
    // pnpm's lockfile walker, which threads one `walked` set through every
    // importer's step.
    let mut visited_packages = HashSet::new();

    for importer_id in importer_ids {
        let mut issues = PeerIssues {
            bad: BTreeMap::new(),
            missing: BTreeMap::new(),
            conflicts: Vec::new(),
            intersections: BTreeMap::new(),
        };

        let mut initial_keys = Vec::new();
        let mut visited_importers = HashSet::new();
        collect_initial_keys(
            importer_id,
            lockfile,
            lockfile_dir,
            &[],
            &mut initial_keys,
            &mut visited_importers,
            &mut issues,
        );

        walk_snapshot(
            initial_keys,
            snapshots,
            packages,
            lockfile_dir,
            &mut visited_packages,
            &mut issues,
        );

        let merged = merge_missing_peers(&issues.missing);
        issues.conflicts = merged.conflicts;
        issues.intersections = merged.intersections;

        result.insert(importer_id.clone(), issues);
    }

    result
}

fn path_is_within(path: &Path, base: &Path) -> bool {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return false;
    };
    canonical_path.starts_with(&canonical_base)
}

/// `base_dir` is the directory the `link:` target is relative to — the
/// importer's directory for importer dependencies, the lockfile directory for
/// snapshot dependencies. Targets escaping `lockfile_dir` are rejected.
fn resolve_link_version(base_dir: &Path, lockfile_dir: &Path, link_target: &str) -> Option<String> {
    let target_dir = base_dir.join(link_target);
    if !path_is_within(&target_dir, lockfile_dir) {
        return None;
    }
    let manifest = PackageManifest::from_path(target_dir.join("package.json")).ok()?;
    package_manifest_version(&manifest)
}

fn package_manifest_version(manifest: &PackageManifest) -> Option<String> {
    manifest.value().get("version").and_then(|version| version.as_str()).map(String::from)
}

/// A workspace package an importer reaches through `link:`, whose own
/// `peerDependencies` the importer has to satisfy.
struct LinkedPackagePeers<'a> {
    importer: &'a pnpm_lockfile::ProjectSnapshot,
    importer_dir: &'a Path,
    lockfile_dir: &'a Path,
    manifest: &'a PackageManifest,
    alias: &'a str,
    linked_version: &'a str,
    issues: &'a mut PeerIssues,
}

fn check_linked_package_peers(inputs: LinkedPackagePeers<'_>) {
    let LinkedPackagePeers {
        importer,
        importer_dir,
        lockfile_dir,
        manifest,
        alias,
        linked_version,
        issues,
    } = inputs;
    let Some(peer_deps) =
        manifest.value().get("peerDependencies").and_then(|deps_val| deps_val.as_object())
    else {
        return;
    };

    let current_parents =
        vec![ParentPkg { name: alias.to_string(), version: linked_version.to_string() }];

    for (peer_name, peer_range_val) in peer_deps {
        let Some(peer_range) = peer_range_val.as_str() else { continue };
        let peer_range = get_peer_version_range(peer_range);
        let is_optional = manifest
            .value()
            .get("peerDependenciesMeta")
            .and_then(|meta_map| meta_map.get(peer_name))
            .and_then(|peer_meta| peer_meta.get("optional"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
        let resolved_ref = importer
            .dependencies
            .as_ref()
            .and_then(|deps| deps.get(&peer_pkg_name))
            .or_else(|| {
                importer.dev_dependencies.as_ref().and_then(|deps| deps.get(&peer_pkg_name))
            })
            .or_else(|| {
                importer.optional_dependencies.as_ref().and_then(|deps| deps.get(&peer_pkg_name))
            });

        match resolved_ref {
            Some(spec) => {
                if let Some(ver_peer) = spec.version.ver_peer() {
                    let version_str = ver_peer.version().to_string();
                    if !satisfies(&version_str, &peer_range) {
                        issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                            parents: current_parents.clone(),
                            optional: is_optional,
                            wanted_range: peer_range.clone(),
                            found_version: version_str,
                            resolved_from: Vec::new(),
                        });
                    }
                } else if let Some(link_target) = spec.version.as_link_target() {
                    let found_version =
                        resolve_link_version(importer_dir, lockfile_dir, link_target)
                            .unwrap_or_else(|| format!("link:{link_target}"));
                    if !satisfies(&found_version, &peer_range) {
                        issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                            parents: current_parents.clone(),
                            optional: is_optional,
                            wanted_range: peer_range.clone(),
                            found_version,
                            resolved_from: Vec::new(),
                        });
                    }
                }
            }
            None => {
                if !is_optional {
                    issues.missing.entry(peer_name.clone()).or_default().push(MissingPeerIssue {
                        parents: current_parents.clone(),
                        optional: is_optional,
                        wanted_range: peer_range.clone(),
                    });
                }
            }
        }
    }
}

fn collect_initial_keys(
    importer_id: &str,
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    parents: &[ParentPkg],
    initial_keys: &mut Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    visited_importers: &mut HashSet<String>,
    issues: &mut PeerIssues,
) {
    if !visited_importers.insert(importer_id.to_string()) {
        return;
    }
    let Some(importer) = lockfile.importers.get(importer_id) else { return };
    let importer_dir = lockfile_dir.join(importer_id);

    let groups =
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];

    for dep_map in groups {
        let Some(dep_map) = dep_map else { continue };
        for (alias, spec) in dep_map {
            if let Some(key) = spec.version.resolved_key(alias) {
                initial_keys.push((key, parents.to_owned()));
            } else if let Some(link_target) = spec.version.as_link_target() {
                // One canonicalization and one manifest read per linked
                // dependency: the version, the peer check, and the
                // recursion all need the same two answers, and this walk
                // now runs on the install path.
                let linked_dir = importer_dir.join(link_target);
                if !path_is_within(&linked_dir, lockfile_dir) {
                    continue;
                }
                let linked_manifest =
                    PackageManifest::from_path(linked_dir.join("package.json")).ok();
                let linked_version = linked_manifest
                    .as_ref()
                    .and_then(package_manifest_version)
                    .unwrap_or_else(|| "0.0.0".to_string());
                let mut next_parents = parents.to_owned();
                next_parents
                    .push(ParentPkg { name: alias.to_string(), version: linked_version.clone() });

                if let Some(linked_manifest) = &linked_manifest {
                    check_linked_package_peers(LinkedPackagePeers {
                        importer,
                        importer_dir: &importer_dir,
                        lockfile_dir,
                        manifest: linked_manifest,
                        alias: &alias.to_string(),
                        linked_version: &linked_version,
                        issues,
                    });
                }

                let linked_importer_id =
                    pnpm_workspace::importer_id_from_root_dir(lockfile_dir, &linked_dir);
                collect_initial_keys(
                    &linked_importer_id,
                    lockfile,
                    lockfile_dir,
                    &next_parents,
                    initial_keys,
                    visited_importers,
                    issues,
                );
            }
        }
    }
}

fn walk_snapshot(
    initial_keys: Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &Path,
    visited: &mut HashSet<PkgNameVerPeer>,
    issues: &mut PeerIssues,
) {
    let mut stack = initial_keys;

    while let Some((key, parents)) = stack.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let pkg_name = key.name.to_string();
        let pkg_version = get_pkg_version(&key, packages);

        let mut current_parents = parents.clone();
        current_parents.push(ParentPkg { name: pkg_name, version: pkg_version });

        let base_key = key.without_peer();
        if let Some(meta) = packages.get(&base_key)
            && let Some(peers) = &meta.peer_dependencies
        {
            let snapshot = snapshots.get(&key);
            for (peer_name, peer_range) in peers {
                let peer_range = get_peer_version_range(peer_range);
                let is_optional = meta
                    .peer_dependencies_meta
                    .as_ref()
                    .and_then(|meta_map| meta_map.get(peer_name))
                    .is_some_and(|peer_meta| peer_meta.optional);

                let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
                let resolved_ref = snapshot.and_then(|snapshot_entry| {
                    snapshot_entry
                        .dependencies
                        .as_ref()
                        .and_then(|deps| deps.get(&peer_pkg_name))
                        .or_else(|| {
                            snapshot_entry
                                .optional_dependencies
                                .as_ref()
                                .and_then(|deps| deps.get(&peer_pkg_name))
                        })
                });

                match resolved_ref {
                    Some(dep_ref) => {
                        if let Some(ver_peer) = dep_ref.ver_peer() {
                            let version_str = ver_peer.version().to_string();
                            if !satisfies(&version_str, &peer_range) {
                                issues.bad.entry(peer_name.clone()).or_default().push(
                                    BadPeerIssue {
                                        parents: current_parents.clone(),
                                        optional: is_optional,
                                        wanted_range: peer_range.clone(),
                                        found_version: version_str,
                                        resolved_from: Vec::new(),
                                    },
                                );
                            }
                        } else if let Some(link_target) = dep_ref.as_link_target() {
                            let found_version =
                                resolve_link_version(lockfile_dir, lockfile_dir, link_target)
                                    .unwrap_or_else(|| format!("link:{link_target}"));
                            if !satisfies(&found_version, &peer_range) {
                                issues.bad.entry(peer_name.clone()).or_default().push(
                                    BadPeerIssue {
                                        parents: current_parents.clone(),
                                        optional: is_optional,
                                        wanted_range: peer_range.clone(),
                                        found_version,
                                        resolved_from: Vec::new(),
                                    },
                                );
                            }
                        }
                    }
                    None => {
                        if !is_optional {
                            issues.missing.entry(peer_name.clone()).or_default().push(
                                MissingPeerIssue {
                                    parents: current_parents.clone(),
                                    optional: is_optional,
                                    wanted_range: peer_range.clone(),
                                },
                            );
                        }
                    }
                }
            }
        }

        if let Some(snapshot) = snapshots.get(&key) {
            let all_deps = snapshot
                .dependencies
                .iter()
                .flat_map(|deps| deps.iter())
                .chain(snapshot.optional_dependencies.iter().flat_map(|deps| deps.iter()));

            for (alias, dep_ref) in all_deps {
                if let Some(child_key) = dep_ref.resolve(alias) {
                    stack.push((child_key, current_parents.clone()));
                }
            }
        }
    }
}

fn get_pkg_version(
    key: &PkgNameVerPeer,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
) -> String {
    let base_key = key.without_peer();
    packages
        .get(&base_key)
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| key.suffix.version().to_string())
}

fn satisfies(version: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    let Ok(parsed_version) = Version::parse(version) else {
        return version == range;
    };
    let Ok(parsed_range) = Range::parse(range) else {
        return version == range;
    };
    if parsed_version.satisfies(&parsed_range) {
        return true;
    }
    if !parsed_version.is_prerelease() {
        return false;
    }
    // pnpm asks semver for `includePrerelease`, which drops the rule
    // that a prerelease only satisfies a comparator carrying a
    // prerelease of its own `major.minor.patch` — `node-semver`'s Rust
    // port applies that rule unconditionally. What is left is the plain
    // bound check, and ordering still holds: `18.3.0-canary` satisfies
    // `^18.0.0`, while `2.0.0-beta.1` stays below `>=2.0.0`.
    parse_range_to_intervals(&preprocess_hyphen_ranges(range)).is_some_and(|intervals| {
        intervals.iter().any(|interval| interval.contains(&parsed_version))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound<Value> {
    Inclusive(Value),
    Exclusive(Value),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    lower: Bound<Version>,
    upper: Bound<Version>,
}

impl Interval {
    fn contains(&self, version: &Version) -> bool {
        let above_lower = match &self.lower {
            Bound::Inclusive(lower) => lower <= version,
            Bound::Exclusive(lower) => lower < version,
            Bound::Unbounded => true,
        };
        let below_upper = match &self.upper {
            Bound::Inclusive(upper) => version <= upper,
            Bound::Exclusive(upper) => version < upper,
            Bound::Unbounded => true,
        };
        above_lower && below_upper
    }
}

/// The bound as a user reads it, without the trailing `-0`.
///
/// For a bound [`derived_upper`] built, the suffix is an implementation
/// detail of prerelease matching. For one the user wrote out it is not
/// — but pnpm drops it too (`semver-range-intersect` renders
/// `intersect("<2.0.0-0", ">=1.0.0")` as `>=1.0.0 <2.0.0`), so telling
/// the two apart here would diverge rather than converge. Only the
/// rendering loses it: matching compares the parsed version, where the
/// suffix still excludes every prerelease of that release.
fn without_derived_suffix(version: &Version) -> String {
    if version.pre_release == [node_semver::Identifier::Numeric(0)] {
        format!("{}.{}.{}", version.major, version.minor, version.patch)
    } else {
        version.to_string()
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let upper = match &self.upper {
            Bound::Inclusive(version) | Bound::Exclusive(version) => {
                without_derived_suffix(version)
            }
            Bound::Unbounded => String::new(),
        };
        match (&self.lower, &self.upper) {
            (Bound::Unbounded, Bound::Unbounded) => write!(formatter, "*"),
            (Bound::Inclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">={version_lower}")
            }
            (Bound::Exclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">{version_lower}")
            }
            (Bound::Unbounded, Bound::Inclusive(_)) => write!(formatter, "<={upper}"),
            (Bound::Unbounded, Bound::Exclusive(_)) => write!(formatter, "<{upper}"),
            (Bound::Inclusive(version_lower), Bound::Inclusive(version_upper)) => {
                if version_lower == version_upper {
                    write!(formatter, "{version_lower}")
                } else {
                    write!(formatter, ">={version_lower} <={upper}")
                }
            }
            (Bound::Inclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">={version_lower} <{upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Inclusive(_)) => {
                write!(formatter, ">{version_lower} <={upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">{version_lower} <{upper}")
            }
        }
    }
}

fn max_lower(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (left_bound, right_bound) {
        (Bound::Unbounded, other) | (other, Bound::Unbounded) => other.clone(),
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            if left_version >= right_version {
                Bound::Inclusive(left_version.clone())
            } else {
                Bound::Inclusive(right_version.clone())
            }
        }
        (Bound::Exclusive(left_version), Bound::Exclusive(right_version)) => {
            if left_version >= right_version {
                Bound::Exclusive(left_version.clone())
            } else {
                Bound::Exclusive(right_version.clone())
            }
        }
        (Bound::Inclusive(left_version), Bound::Exclusive(right_version)) => {
            if left_version > right_version {
                Bound::Inclusive(left_version.clone())
            } else {
                Bound::Exclusive(right_version.clone())
            }
        }
        (Bound::Exclusive(left_version), Bound::Inclusive(right_version)) => {
            if left_version >= right_version {
                Bound::Exclusive(left_version.clone())
            } else {
                Bound::Inclusive(right_version.clone())
            }
        }
    }
}

fn min_upper(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (left_bound, right_bound) {
        (Bound::Unbounded, other) | (other, Bound::Unbounded) => other.clone(),
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            if left_version <= right_version {
                Bound::Inclusive(left_version.clone())
            } else {
                Bound::Inclusive(right_version.clone())
            }
        }
        (Bound::Exclusive(left_version), Bound::Exclusive(right_version)) => {
            if left_version <= right_version {
                Bound::Exclusive(left_version.clone())
            } else {
                Bound::Exclusive(right_version.clone())
            }
        }
        (Bound::Inclusive(left_version), Bound::Exclusive(right_version)) => {
            if left_version < right_version {
                Bound::Inclusive(left_version.clone())
            } else {
                Bound::Exclusive(right_version.clone())
            }
        }
        (Bound::Exclusive(left_version), Bound::Inclusive(right_version)) => {
            if left_version <= right_version {
                Bound::Exclusive(left_version.clone())
            } else {
                Bound::Inclusive(right_version.clone())
            }
        }
    }
}

fn is_valid_interval(lower: &Bound<Version>, upper: &Bound<Version>) -> bool {
    match (lower, upper) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version <= right_version
        }
        (Bound::Inclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
    }
}

fn normalize_version_str(version_raw: &str) -> String {
    let version_raw = version_raw.trim();
    let version_parts: Vec<&str> = version_raw.split('.').collect();
    match version_parts.len() {
        1 => {
            let major = version_parts[0].replace(['x', 'X', '*'], "0");
            if major.chars().all(|character| character.is_ascii_digit()) {
                format!("{major}.0.0")
            } else {
                version_raw.to_string()
            }
        }
        2 => {
            let major = version_parts[0].replace(['x', 'X', '*'], "0");
            let minor = version_parts[1].replace(['x', 'X', '*'], "0");
            if major.chars().all(|character| character.is_ascii_digit())
                && minor.chars().all(|character| character.is_ascii_digit())
            {
                format!("{major}.{minor}.0")
            } else {
                version_raw.to_string()
            }
        }
        _ => {
            let major = version_parts[0].replace(['x', 'X', '*'], "0");
            let minor = version_parts[1].replace(['x', 'X', '*'], "0");
            let patch = version_parts[2].replace(['x', 'X', '*'], "0");
            if major.chars().all(|character| character.is_ascii_digit())
                && minor.chars().all(|character| character.is_ascii_digit())
                && patch.chars().all(|character| character.is_ascii_digit())
            {
                let rest = if version_parts.len() > 3 {
                    format!(".{}", version_parts[3..].join("."))
                } else {
                    String::new()
                };
                format!("{major}.{minor}.{patch}{rest}")
            } else {
                version_raw.to_string()
            }
        }
    }
}

/// How many of `major.minor.patch` a range's version actually pins.
/// `1` and `1.x` pin one, `1.2` pins two, `1.2.3` pins three. npm's
/// comparators widen to the next unpinned level — `~1` reaches `2.0.0`,
/// not `1.1.0` — so the count has to survive the padding
/// [`normalize_version_str`] applies.
fn version_specificity(version_raw: &str) -> usize {
    let mut specificity = 0;
    for part in version_raw.trim().split('.') {
        let head = part.split(['-', '+']).next().unwrap_or(part);
        if head.is_empty() || head.chars().all(|character| matches!(character, 'x' | 'X' | '*')) {
            break;
        }
        specificity += 1;
        if specificity == 3 {
            break;
        }
    }
    specificity
}

fn at(major: u64, minor: u64, patch: u64) -> Version {
    Version { major, minor, patch, build: Vec::new(), pre_release: Vec::new() }
}

/// An upper bound npm derived rather than the user writing it out:
/// `^1.2.3` desugars to `<2.0.0-0`, not `<2.0.0`, so that no prerelease
/// of 2.0.0 slips in. A bound the user spelled in full (`<2.0.0`) keeps
/// its plain form and does admit `2.0.0-rc.1`. The suffix is dropped
/// again when the interval is rendered — see [`Interval`]'s `Display`.
fn derived_upper(version: Version) -> Version {
    Version { pre_release: vec![node_semver::Identifier::Numeric(0)], ..version }
}

/// The exclusive upper bound of the level `specificity` leaves
/// unpinned: the next major for a bare major, the next minor for
/// `major.minor`.
fn next_unpinned(version: &Version, specificity: usize) -> Version {
    let next = if specificity >= 2 {
        at(version.major, version.minor + 1, 0)
    } else {
        at(version.major + 1, 0, 0)
    };
    derived_upper(next)
}

fn parse_comparator(comparator: &str) -> Option<Interval> {
    let comparator = comparator.trim();
    if comparator == "*" || comparator.is_empty() {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }

    let (operator, version_str) = if let Some(rest) = comparator.strip_prefix(">=") {
        ("=>", rest)
    } else if let Some(rest) = comparator.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = comparator.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = comparator.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = comparator.strip_prefix('^') {
        ("^", rest)
    } else if let Some(rest) = comparator.strip_prefix('~') {
        ("~", rest)
    } else {
        ("=", comparator)
    };

    let specificity = version_specificity(version_str);
    // Nothing is pinned (`x`, `~x`, `^*`): every comparator over it
    // admits every version.
    if specificity == 0 {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }
    let normalized = normalize_version_str(version_str);
    let version = Version::parse(&normalized).ok()?;

    match operator {
        "=" if specificity == 3 => Some(Interval {
            lower: Bound::Inclusive(version.clone()),
            upper: Bound::Inclusive(version),
        }),
        // A partial bare version is npm's implicit range: `1.2` is
        // every 1.2.x, not the single version 1.2.0.
        "=" => Some(Interval {
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        "=>" => Some(Interval { lower: Bound::Inclusive(version), upper: Bound::Unbounded }),
        ">" if specificity == 3 => {
            Some(Interval { lower: Bound::Exclusive(version), upper: Bound::Unbounded })
        }
        // `>1.2` excludes all of 1.2.x, so it starts at 1.3.0.
        ">" => Some(Interval {
            lower: Bound::Inclusive(next_unpinned(&version, specificity)),
            upper: Bound::Unbounded,
        }),
        "<=" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Inclusive(version) })
        }
        // `<=1.2` admits all of 1.2.x.
        "<=" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
        }),
        "<" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Exclusive(version) })
        }
        // `<1.2` excludes all of 1.2.x, prereleases included.
        "<" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(derived_upper(version)),
        }),
        "^" => {
            let upper_version = if specificity == 1 || version.major > 0 {
                at(version.major + 1, 0, 0)
            } else if specificity == 2 || version.minor > 0 {
                at(0, version.minor + 1, 0)
            } else {
                at(0, 0, version.patch + 1)
            };
            let upper_version = derived_upper(upper_version);
            Some(Interval {
                lower: Bound::Inclusive(version),
                upper: Bound::Exclusive(upper_version),
            })
        }
        "~" => Some(Interval {
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        _ => None,
    }
}

fn preprocess_hyphen_ranges(range: &str) -> String {
    let mut parts = Vec::new();
    for part in range.split("||") {
        let part = part.trim();
        if let Some((start, end)) = part.split_once(" - ") {
            parts.push(format!(">={} <={}", start.trim(), end.trim()));
        } else {
            parts.push(part.to_string());
        }
    }
    parts.join(" || ")
}

fn parse_range_to_intervals(range: &str) -> Option<Vec<Interval>> {
    let mut intervals = Vec::new();
    for part in range.split("||") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut part_interval = Interval { lower: Bound::Unbounded, upper: Bound::Unbounded };
        for comp in part.split_whitespace() {
            let comp_interval = parse_comparator(comp)?;
            let lower = max_lower(&part_interval.lower, &comp_interval.lower);
            let upper = min_upper(&part_interval.upper, &comp_interval.upper);
            if !is_valid_interval(&lower, &upper) {
                part_interval = Interval {
                    lower: Bound::Inclusive(Version::parse("0.0.0").unwrap()),
                    upper: Bound::Exclusive(Version::parse("0.0.0").unwrap()),
                };
                break;
            }
            part_interval = Interval { lower, upper };
        }
        if is_valid_interval(&part_interval.lower, &part_interval.upper) {
            intervals.push(part_interval);
        }
    }
    if intervals.is_empty() { None } else { Some(intervals) }
}

fn intersect_intervals(left_intervals: &[Interval], right_intervals: &[Interval]) -> Vec<Interval> {
    let mut result = Vec::new();
    for left_interval in left_intervals {
        for right_interval in right_intervals {
            let lower = max_lower(&left_interval.lower, &right_interval.lower);
            let upper = min_upper(&left_interval.upper, &right_interval.upper);
            if is_valid_interval(&lower, &upper) {
                result.push(Interval { lower, upper });
            }
        }
    }
    result
}

fn intersect_multiple_ranges(version_ranges: &[String]) -> Option<String> {
    if version_ranges.is_empty() {
        return Some("*".to_string());
    }
    let mut current_intervals =
        parse_range_to_intervals(&preprocess_hyphen_ranges(&version_ranges[0]))?;
    for range in &version_ranges[1..] {
        let next_intervals = parse_range_to_intervals(&preprocess_hyphen_ranges(range))?;
        current_intervals = intersect_intervals(&current_intervals, &next_intervals);
        if current_intervals.is_empty() {
            return None;
        }
    }
    Some(
        current_intervals
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" || "),
    )
}

fn merge_missing_peers(missing: &BTreeMap<String, Vec<MissingPeerIssue>>) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut intersections = BTreeMap::new();

    for (peer_name, issues) in missing {
        if issues.iter().all(|issue| issue.optional) {
            continue;
        }
        if issues.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let ranges: Vec<&str> = issues.iter().map(|issue| issue.wanted_range.as_str()).collect();
        let unique: HashSet<&&str> = ranges.iter().collect();
        if unique.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let range_owned: Vec<String> =
            issues.iter().map(|issue| issue.wanted_range.clone()).collect();
        if let Some(intersection_str) = intersect_multiple_ranges(&range_owned) {
            intersections.insert(peer_name.clone(), intersection_str);
        } else {
            conflicts.push(peer_name.clone());
        }
    }

    MergeResult { conflicts, intersections }
}

struct MergeResult {
    conflicts: Vec<String>,
    intersections: BTreeMap<String, String>,
}

#[must_use]
pub fn filter_peer_issues(
    mut issues: IssuesByProjects,
    rules: &PeerDependencyRules,
) -> IssuesByProjects {
    if rules.ignore_missing.is_none()
        && rules.allow_any.is_none()
        && rules.allowed_versions.is_none()
    {
        return issues;
    }

    let ignore_missing_pats = rules.ignore_missing.clone().unwrap_or_default();
    let allow_any_pats = rules.allow_any.clone().unwrap_or_default();
    let allowed_versions_map = rules.allowed_versions.clone().unwrap_or_default();

    let (allow_all_matcher, allow_by_parent) = parse_allowed_versions(&allowed_versions_map);
    let ignore_missing_matcher = pnpm_config::matcher::create_matcher(&ignore_missing_pats);
    let allow_any_matcher_rule = pnpm_config::matcher::create_matcher(&allow_any_pats);

    for project_issues in issues.values_mut() {
        let mut filtered_missing: BTreeMap<String, Vec<MissingPeerIssue>> = BTreeMap::new();
        let mut filtered_bad: BTreeMap<String, Vec<BadPeerIssue>> = BTreeMap::new();

        for (peer_name, peer_issues) in &project_issues.missing {
            if ignore_missing_matcher.matches(peer_name)
                || peer_issues.iter().all(|issue| issue.optional)
            {
                continue;
            }
            filtered_missing.insert(peer_name.clone(), peer_issues.clone());
        }

        for (peer_name, peer_issues) in &project_issues.bad {
            if allow_any_matcher_rule.matches(peer_name) {
                continue;
            }
            let remaining: Vec<BadPeerIssue> = peer_issues
                .iter()
                .filter(|issue| {
                    if let Some(ranges) = allow_all_matcher.get(peer_name)
                        && ranges.iter().any(|range| satisfies(&issue.found_version, range))
                    {
                        return false;
                    }
                    if let Some(declaring_parent) = issue.parents.last()
                        && let Some(rules) = allow_by_parent.get(&declaring_parent.name)
                    {
                        for rule in rules {
                            let range_matches = match &rule.parent_range {
                                Some(range) => satisfies(&declaring_parent.version, range),
                                None => true,
                            };
                            if range_matches
                                && let Some(ranges) = rule.peer_rules.get(peer_name)
                                && ranges.iter().any(|range| satisfies(&issue.found_version, range))
                            {
                                return false;
                            }
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            if !remaining.is_empty() {
                filtered_bad.insert(peer_name.clone(), remaining);
            }
        }

        project_issues.missing = filtered_missing;
        project_issues.bad = filtered_bad;
        let merged = merge_missing_peers(&project_issues.missing);
        project_issues.conflicts = merged.conflicts;
        project_issues.intersections = merged.intersections;
    }

    issues
}

type AllowAllMatcher = HashMap<String, Vec<String>>;
type AllowByParentMatcher = HashMap<String, Vec<ParentRule>>;

struct ParentRule {
    parent_range: Option<String>,
    peer_rules: HashMap<String, Vec<String>>,
}

fn parse_allowed_versions(
    allowed: &BTreeMap<String, String>,
) -> (AllowAllMatcher, AllowByParentMatcher) {
    let mut match_all: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_parent: AllowByParentMatcher = HashMap::new();

    for (selector, spec) in allowed {
        if let Some((parent, target)) = selector.split_once('>') {
            let parsed_parent = parse_wanted_dependency(parent.trim());
            let parent_name = parsed_parent.alias.unwrap_or_else(|| parent.trim().to_string());
            let parent_range = parsed_parent.bare_specifier;

            let parsed_peer = parse_wanted_dependency(target.trim());
            let peer_name = parsed_peer.alias.unwrap_or_else(|| target.trim().to_string());

            let ranges: Vec<String> = spec.split("||").map(|seg| seg.trim().to_string()).collect();

            let parent_entry = by_parent.entry(parent_name).or_default();
            if let Some(rule) =
                parent_entry.iter_mut().find(|rule_entry| rule_entry.parent_range == parent_range)
            {
                rule.peer_rules.entry(peer_name).or_default().extend(ranges);
            } else {
                let mut peer_rules = HashMap::new();
                peer_rules.insert(peer_name, ranges);
                parent_entry.push(ParentRule { parent_range, peer_rules });
            }
        } else {
            let parsed = parse_wanted_dependency(selector);
            let target_name = parsed.alias.unwrap_or_else(|| selector.clone());
            let ranges: Vec<String> = spec.split("||").map(|seg| seg.trim().to_string()).collect();
            match_all.entry(target_name).or_default().extend(ranges);
        }
    }

    (match_all, by_parent)
}

#[must_use]
pub fn render_peer_issues(issues_by_projects: &IssuesByProjects) -> String {
    let mut sections: Vec<String> = Vec::new();

    for project_issues in issues_by_projects.values() {
        for (peer_name, issues) in &project_issues.bad {
            let peer_name_bold = bold(peer_name);
            let header = format!("{} {}", yellow_bright("✕ unmet peer"), peer_name_bold);
            let groups = group_by_found_version(issues);
            for (found_version, group) in &groups {
                let installed = format!("  {} {}", cyan("Installed:"), dim(found_version));
                sections.push(format!("{}\n{}\n{}", header, installed, format_required_by(group)));
            }
        }

        for (peer_name, issues) in &project_issues.missing {
            let is_conflict = project_issues.conflicts.contains(peer_name);
            if !project_issues.intersections.contains_key(peer_name) && !is_conflict {
                continue;
            }
            let peer_name_bold = bold(peer_name);
            let header = if is_conflict {
                format!("{} {}", red("✕ conflicting peer"), peer_name_bold)
            } else {
                format!("{} {}", red("✕ missing peer"), peer_name_bold)
            };
            sections.push(format!("{}\n{}", header, format_required_by(issues)));
        }
    }

    if sections.is_empty() {
        return String::new();
    }
    sections.join("\n\n")
}

fn format_required_by(issues: &[impl RequiredByIssue]) -> String {
    let mut by_range: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issue in issues {
        let declaring = issue.parents().last().cloned().unwrap_or_default();
        let pkg = if declaring.name.is_empty() {
            "<unknown>".to_string()
        } else {
            format!("{}@{}", declaring.name, declaring.version)
        };
        let pkgs = by_range.entry(issue.wanted_range().to_string()).or_default();
        if !pkgs.contains(&pkg) {
            pkgs.push(pkg);
        }
    }

    let mut lines: Vec<String> = vec![format!("  {}", cyan("Wanted:"))];
    for (range, pkgs) in &by_range {
        lines.push(format!("    {}{}", cyan_bright(&format_range(range)), cyan(":")));
        for pkg in pkgs {
            lines.push(format!("      {}", dim(pkg)));
        }
    }
    lines.join("\n")
}

trait RequiredByIssue {
    fn parents(&self) -> &[ParentPkg];
    fn wanted_range(&self) -> &str;
}

impl RequiredByIssue for MissingPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

impl RequiredByIssue for BadPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

fn group_by_found_version(issues: &[BadPeerIssue]) -> BTreeMap<String, Vec<BadPeerIssue>> {
    let mut groups: BTreeMap<String, Vec<BadPeerIssue>> = BTreeMap::new();
    for issue in issues {
        groups.entry(issue.found_version.clone()).or_default().push(issue.clone());
    }
    groups
}

fn format_range(range: &str) -> String {
    if range.contains(' ') || range == "*" { format!(r#""{range}""#) } else { range.to_string() }
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dim(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn yellow_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_yellow()).to_string()
}

fn red(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn cyan(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.cyan()).to_string()
}

fn cyan_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}

#[cfg(test)]
mod tests;
