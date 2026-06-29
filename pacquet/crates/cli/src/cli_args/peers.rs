use std::collections::{HashMap, HashSet};
use std::fmt;

use clap::Args;
use miette::{Context, IntoDiagnostic};
use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;

use pacquet_config::{Config, PeerDependencyRules};
use pacquet_lockfile::{Lockfile, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotEntry};
use pacquet_package_manifest::PackageManifest;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;

use crate::cli_args::sanitize::sanitize;

#[derive(Debug, Default, Clone, Serialize)]
struct ParentPkg {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Serialize)]
struct MissingPeerIssue {
    parents: Vec<ParentPkg>,
    optional: bool,
    #[serde(rename = "wantedRange")]
    wanted_range: String,
}

#[derive(Debug, Clone, Serialize)]
struct BadPeerIssue {
    parents: Vec<ParentPkg>,
    optional: bool,
    #[serde(rename = "wantedRange")]
    wanted_range: String,
    #[serde(rename = "foundVersion")]
    found_version: String,
    #[serde(rename = "resolvedFrom")]
    resolved_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PeerIssues {
    bad: HashMap<String, Vec<BadPeerIssue>>,
    missing: HashMap<String, Vec<MissingPeerIssue>>,
    conflicts: Vec<String>,
    intersections: HashMap<String, String>,
}

type IssuesByProjects = HashMap<String, PeerIssues>;

#[derive(Debug, Args)]
pub struct PeersArgs {
    #[clap(long)]
    pub json: bool,

    #[clap(long)]
    pub lockfile_only: bool,
}

impl PeersArgs {
    pub fn run(
        self,
        config: &Config,
        dir: &std::path::Path,
        recursive: bool,
    ) -> miette::Result<()> {
        let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);

        let lockfile = if self.lockfile_only {
            Lockfile::load_wanted_from_dir(lockfile_dir)
        } else {
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
                Ok(Some(lf)) => Ok(Some(lf)),
                Ok(None) => Lockfile::load_wanted_from_dir(lockfile_dir),
                Err(e) => Err(e),
            }
        }
        .into_diagnostic()
        .wrap_err("load lockfile")?;

        let Some(lockfile) = lockfile else {
            if self.json {
                println!("{{}}");
            } else {
                let dir_str = lockfile_dir.display().to_string();
                println!("No lockfile found in {}", sanitize(&dir_str));
            }
            return Ok(());
        };

        let issues = check_peer_dependencies_from_lockfile(&lockfile, lockfile_dir, dir, recursive);
        let issues = filter_peer_issues(issues, &config.peer_dependency_rules);

        let no_issues = issues.values().all(|pi| pi.bad.is_empty() && pi.missing.is_empty());

        if self.json {
            let output = serde_json::to_string_pretty(&issues)
                .into_diagnostic()
                .wrap_err("serialize issues to JSON")?;
            println!("{output}");
        } else if no_issues {
            println!("No peer dependency issues found");
        } else {
            println!("Issues with peer dependencies found\n");
            print!("{}", render_peer_issues(&issues));
        }

        if !no_issues {
            #[allow(clippy::exit, reason = "peers exits non-zero on issues, mirroring pnpm")]
            std::process::exit(1);
        }

        Ok(())
    }
}

fn check_peer_dependencies_from_lockfile(
    lockfile: &Lockfile,
    lockfile_dir: &std::path::Path,
    dir: &std::path::Path,
    recursive: bool,
) -> IssuesByProjects {
    let packages = lockfile.packages.as_ref();
    let snapshots = lockfile.snapshots.as_ref();
    let Some(snapshots) = snapshots else { return HashMap::new() };
    let Some(packages) = packages else { return HashMap::new() };

    let mut importer_ids: Vec<String> = if recursive {
        lockfile.importers.keys().cloned().collect()
    } else {
        vec![resolve_importer_id(lockfile_dir, dir)]
    };
    importer_ids.sort();

    let mut result: IssuesByProjects = HashMap::new();

    for importer_id in &importer_ids {
        let Some(_importer) = lockfile.importers.get(importer_id.as_str()) else { continue };

        let mut issues = PeerIssues {
            bad: HashMap::new(),
            missing: HashMap::new(),
            conflicts: Vec::new(),
            intersections: HashMap::new(),
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

        walk_snapshot(initial_keys, snapshots, packages, lockfile_dir, &mut issues);

        let merged = merge_missing_peers(&issues.missing);
        issues.conflicts = merged.conflicts;
        issues.intersections = merged.intersections;

        result.insert(importer_id.clone(), issues);
    }

    result
}

fn resolve_importer_id(lockfile_dir: &std::path::Path, dir: &std::path::Path) -> String {
    if dir == lockfile_dir {
        Lockfile::ROOT_IMPORTER_KEY.to_string()
    } else {
        dir.strip_prefix(lockfile_dir)
            .ok()
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| Lockfile::ROOT_IMPORTER_KEY.to_string())
    }
}

fn path_is_within(path: &std::path::Path, base: &std::path::Path) -> bool {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return false;
    };
    canonical_path.starts_with(&canonical_base)
}

fn resolve_link_version(lockfile_dir: &std::path::Path, link_target: &str) -> Option<String> {
    let target_dir = lockfile_dir.join(link_target);
    if !path_is_within(&target_dir, lockfile_dir) {
        return None;
    }
    let manifest_path = target_dir.join("package.json");
    PackageManifest::from_path(manifest_path).ok().and_then(|manifest| {
        manifest.value().get("version").and_then(|v| v.as_str()).map(String::from)
    })
}

fn check_linked_package_peers(
    importer_id: &str,
    importer: &pacquet_lockfile::ProjectSnapshot,
    link_target: &str,
    alias: &str,
    linked_version: &str,
    lockfile_dir: &std::path::Path,
    issues: &mut PeerIssues,
) {
    let linked_pkg_dir = lockfile_dir.join(importer_id).join(link_target);
    if !path_is_within(&linked_pkg_dir, lockfile_dir) {
        return;
    }
    let manifest_path = linked_pkg_dir.join("package.json");
    let Ok(manifest) = PackageManifest::from_path(manifest_path) else { return };
    let Some(peer_deps) = manifest.value().get("peerDependencies").and_then(|v| v.as_object())
    else {
        return;
    };

    let current_parents =
        vec![ParentPkg { name: alias.to_string(), version: linked_version.to_string() }];

    for (peer_name, peer_range_val) in peer_deps {
        let Some(peer_range) = peer_range_val.as_str() else { continue };
        let is_optional = manifest
            .value()
            .get("peerDependenciesMeta")
            .and_then(|m| m.get(peer_name))
            .and_then(|m| m.get("optional"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
        let resolved_ref = importer
            .dependencies
            .as_ref()
            .and_then(|d| d.get(&peer_pkg_name))
            .or_else(|| importer.dev_dependencies.as_ref().and_then(|d| d.get(&peer_pkg_name)))
            .or_else(|| {
                importer.optional_dependencies.as_ref().and_then(|d| d.get(&peer_pkg_name))
            });

        match resolved_ref {
            Some(spec) => {
                if let Some(ver_peer) = spec.version.ver_peer() {
                    let version_str = ver_peer.version().to_string();
                    if !satisfies(&version_str, peer_range) {
                        issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                            parents: current_parents.clone(),
                            optional: is_optional,
                            wanted_range: peer_range.to_string(),
                            found_version: version_str,
                            resolved_from: Vec::new(),
                        });
                    }
                } else if let Some(link_target) = spec.version.as_link_target() {
                    let found_version = resolve_link_version(lockfile_dir, link_target)
                        .unwrap_or_else(|| format!("link:{link_target}"));
                    if !satisfies(&found_version, peer_range) {
                        issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                            parents: current_parents.clone(),
                            optional: is_optional,
                            wanted_range: peer_range.to_string(),
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
                        wanted_range: peer_range.to_string(),
                    });
                }
            }
        }
    }
}

fn collect_initial_keys(
    importer_id: &str,
    lockfile: &Lockfile,
    lockfile_dir: &std::path::Path,
    parents: &[ParentPkg],
    initial_keys: &mut Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    visited_importers: &mut HashSet<String>,
    issues: &mut PeerIssues,
) {
    if !visited_importers.insert(importer_id.to_string()) {
        return;
    }
    let Some(importer) = lockfile.importers.get(importer_id) else { return };

    let groups = [
        (&importer.dependencies, false),
        (&importer.dev_dependencies, false),
        (&importer.optional_dependencies, true),
    ];

    for (dep_map, _is_optional) in &groups {
        let Some(dep_map) = dep_map else { continue };
        for (alias, spec) in dep_map {
            if let Some(key) = spec.version.resolved_key(alias) {
                initial_keys.push((key, parents.to_owned()));
            } else if let Some(link_target) = spec.version.as_link_target() {
                let linked_version = resolve_link_version(lockfile_dir, link_target)
                    .unwrap_or_else(|| "0.0.0".to_string());
                let mut next_parents = parents.to_owned();
                next_parents
                    .push(ParentPkg { name: alias.to_string(), version: linked_version.clone() });

                check_linked_package_peers(
                    importer_id,
                    importer,
                    link_target,
                    &alias.to_string(),
                    &linked_version,
                    lockfile_dir,
                    issues,
                );

                let linked_importer_path = lockfile_dir.join(importer_id).join(link_target);
                if !path_is_within(&linked_importer_path, lockfile_dir) {
                    continue;
                }
                let linked_importer_id = resolve_importer_id(lockfile_dir, &linked_importer_path);
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
    lockfile_dir: &std::path::Path,
    issues: &mut PeerIssues,
) {
    let mut visited = HashSet::new();
    let mut stack = initial_keys;

    while let Some((key, parents)) = stack.pop() {
        let pkg_name = key.name.to_string();
        let pkg_version = get_pkg_version(&key, packages);

        let mut current_parents = parents.clone();
        current_parents.push(ParentPkg { name: pkg_name, version: pkg_version });

        // 1. Evaluate peer dependencies of the current package
        let base_key = key.without_peer();
        if let Some(meta) = packages.get(&base_key)
            && let Some(peers) = &meta.peer_dependencies
        {
            let snapshot = snapshots.get(&key);
            for (peer_name, peer_range) in peers {
                let is_optional = meta
                    .peer_dependencies_meta
                    .as_ref()
                    .and_then(|meta_map| meta_map.get(peer_name))
                    .is_some_and(|peer_meta| peer_meta.optional);

                let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
                let resolved_ref = snapshot.and_then(|s| {
                    s.dependencies.as_ref().and_then(|deps| deps.get(&peer_pkg_name)).or_else(
                        || {
                            s.optional_dependencies
                                .as_ref()
                                .and_then(|deps| deps.get(&peer_pkg_name))
                        },
                    )
                });

                match resolved_ref {
                    Some(dep_ref) => {
                        if let Some(ver_peer) = dep_ref.ver_peer() {
                            let version_str = ver_peer.version().to_string();
                            if !satisfies(&version_str, peer_range) {
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
                            let found_version = resolve_link_version(lockfile_dir, link_target)
                                .unwrap_or_else(|| format!("link:{link_target}"));
                            if !satisfies(&found_version, peer_range) {
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

        // 2. Only recurse if we haven't visited this node yet
        if !visited.insert(key.clone()) {
            continue;
        }

        // 3. Push children to stack
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
    let base = Version {
        major: parsed_version.major,
        minor: parsed_version.minor,
        patch: parsed_version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    base.satisfies(&parsed_range)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound<V> {
    Inclusive(V),
    Exclusive(V),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    lower: Bound<Version>,
    upper: Bound<Version>,
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.lower, &self.upper) {
            (Bound::Unbounded, Bound::Unbounded) => write!(formatter, "*"),
            (Bound::Inclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">={version_lower}")
            }
            (Bound::Exclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">{version_lower}")
            }
            (Bound::Unbounded, Bound::Inclusive(version_upper)) => {
                write!(formatter, "<={version_upper}")
            }
            (Bound::Unbounded, Bound::Exclusive(version_upper)) => {
                write!(formatter, "<{version_upper}")
            }
            (Bound::Inclusive(version_lower), Bound::Inclusive(version_upper)) => {
                if version_lower == version_upper {
                    write!(formatter, "{version_lower}")
                } else {
                    write!(formatter, ">={version_lower} <={version_upper}")
                }
            }
            (Bound::Inclusive(version_lower), Bound::Exclusive(version_upper)) => {
                write!(formatter, ">={version_lower} <{version_upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Inclusive(version_upper)) => {
                write!(formatter, ">{version_lower} <={version_upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Exclusive(version_upper)) => {
                write!(formatter, ">{version_lower} <{version_upper}")
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

    let normalized = normalize_version_str(version_str);
    let version = Version::parse(&normalized).ok()?;

    match operator {
        "=" => Some(Interval {
            lower: Bound::Inclusive(version.clone()),
            upper: Bound::Inclusive(version),
        }),
        "=>" => Some(Interval { lower: Bound::Inclusive(version), upper: Bound::Unbounded }),
        ">" => Some(Interval { lower: Bound::Exclusive(version), upper: Bound::Unbounded }),
        "<=" => Some(Interval { lower: Bound::Unbounded, upper: Bound::Inclusive(version) }),
        "<" => Some(Interval { lower: Bound::Unbounded, upper: Bound::Exclusive(version) }),
        "^" => {
            let upper_version = if version.major > 0 {
                Version {
                    major: version.major + 1,
                    minor: 0,
                    patch: 0,
                    build: Vec::new(),
                    pre_release: Vec::new(),
                }
            } else if version.minor > 0 {
                Version {
                    major: 0,
                    minor: version.minor + 1,
                    patch: 0,
                    build: Vec::new(),
                    pre_release: Vec::new(),
                }
            } else {
                Version {
                    major: 0,
                    minor: 0,
                    patch: version.patch + 1,
                    build: Vec::new(),
                    pre_release: Vec::new(),
                }
            };
            Some(Interval {
                lower: Bound::Inclusive(version),
                upper: Bound::Exclusive(upper_version),
            })
        }
        "~" => {
            let upper_version = Version {
                major: version.major,
                minor: version.minor + 1,
                patch: 0,
                build: Vec::new(),
                pre_release: Vec::new(),
            };
            Some(Interval {
                lower: Bound::Inclusive(version),
                upper: Bound::Exclusive(upper_version),
            })
        }
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

#[cfg(test)]
fn have_common_version(version_ranges: &[String]) -> bool {
    intersect_multiple_ranges(version_ranges).is_some()
}

fn merge_missing_peers(missing: &HashMap<String, Vec<MissingPeerIssue>>) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut intersections = HashMap::new();

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
    intersections: HashMap<String, String>,
}

fn filter_peer_issues(
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
    let allowed_versions_map: HashMap<String, String> =
        rules.allowed_versions.clone().unwrap_or_default().into_iter().collect();

    let (allow_all_matcher, allow_by_parent) = parse_allowed_versions(&allowed_versions_map);
    let ignore_missing_matcher = pacquet_config::matcher::create_matcher(&ignore_missing_pats);
    let allow_any_matcher_rule = pacquet_config::matcher::create_matcher(&allow_any_pats);

    for project_issues in issues.values_mut() {
        let mut filtered_missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
        let mut filtered_bad: HashMap<String, Vec<BadPeerIssue>> = HashMap::new();

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
    allowed: &HashMap<String, String>,
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
            if let Some(rule) = parent_entry.iter_mut().find(|r| r.parent_range == parent_range) {
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

fn render_peer_issues(issues_by_projects: &IssuesByProjects) -> String {
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
    let mut by_range: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for issue in issues {
        let declaring = issue.parents().last().cloned().unwrap_or_default();
        let pkg = if declaring.name.is_empty() {
            "<unknown>".to_string()
        } else {
            format!("{}@{}", declaring.name, declaring.version)
        };
        by_range.entry(issue.wanted_range().to_string()).or_default().push(pkg);
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

fn group_by_found_version(issues: &[BadPeerIssue]) -> HashMap<String, Vec<BadPeerIssue>> {
    let mut groups: HashMap<String, Vec<BadPeerIssue>> = HashMap::new();
    for issue in issues {
        groups.entry(issue.found_version.clone()).or_default().push(issue.clone());
    }
    groups
}

fn format_range(range: &str) -> String {
    if range.contains(' ') || range == "*" { format!("\"{range}\"") } else { range.to_string() }
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
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.yellow()).to_string()
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
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.cyan()).to_string()
}

#[cfg(test)]
mod tests;
