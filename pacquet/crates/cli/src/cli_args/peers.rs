use std::collections::{HashMap, HashSet};

use clap::Args;
use miette::{Context, IntoDiagnostic};
use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;

use pacquet_config::{Config, PeerDependencyRules};
use pacquet_lockfile::{Lockfile, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotEntry};

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
            println!("No lockfile found in {}", lockfile_dir.display());
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

    let importer_ids: Vec<String> = if recursive || config_has_workspace(lockfile_dir, dir) {
        lockfile.importers.keys().cloned().collect()
    } else {
        vec![resolve_importer_id(lockfile_dir, dir)]
    };

    let mut result: IssuesByProjects = HashMap::new();

    for importer_id in &importer_ids {
        let Some(importer) = lockfile.importers.get(importer_id.as_str()) else { continue };

        let mut issues = PeerIssues {
            bad: HashMap::new(),
            missing: HashMap::new(),
            conflicts: Vec::new(),
            intersections: HashMap::new(),
        };

        let mut visited = HashSet::new();

        let groups = [
            (&importer.dependencies, false),
            (&importer.dev_dependencies, false),
            (&importer.optional_dependencies, true),
        ];

        for (dep_map, _is_optional) in &groups {
            let Some(dep_map) = dep_map else { continue };
            for (alias, spec) in dep_map {
                if let Some(key) = spec.version.resolved_key(alias) {
                    walk_snapshot(&key, snapshots, packages, &[], &mut issues, &mut visited);
                }
            }
        }

        let merged = merge_missing_peers(&issues.missing);
        issues.conflicts = merged.conflicts;
        issues.intersections = merged.intersections;

        result.insert(importer_id.clone(), issues);
    }

    result
}

fn config_has_workspace(lockfile_dir: &std::path::Path, dir: &std::path::Path) -> bool {
    lockfile_dir != dir || lockfile_dir.parent().is_none()
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

fn walk_snapshot(
    key: &PkgNameVerPeer,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    parents: &[ParentPkg],
    issues: &mut PeerIssues,
    visited: &mut HashSet<PkgNameVerPeer>,
) {
    if !visited.insert(key.clone()) {
        return;
    }

    let Some(snapshot) = snapshots.get(key) else { return };

    let pkg_name = key.name.to_string();
    let pkg_version = get_pkg_version(key, packages);

    let mut current_parents = parents.to_vec();
    current_parents.push(ParentPkg { name: pkg_name, version: pkg_version });

    let base_key = key.without_peer();
    if let Some(meta) = packages.get(&base_key)
        && let Some(peers) = &meta.peer_dependencies
    {
        for (peer_name, peer_range) in peers {
            let is_optional = meta
                .peer_dependencies_meta
                .as_ref()
                .and_then(|m| m.get(peer_name))
                .is_some_and(|m| m.optional);

            let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
            let resolved_ref =
                snapshot.dependencies.as_ref().and_then(|deps| deps.get(&peer_pkg_name)).or_else(
                    || {
                        snapshot
                            .optional_dependencies
                            .as_ref()
                            .and_then(|deps| deps.get(&peer_pkg_name))
                    },
                );

            match resolved_ref {
                Some(dep_ref) => {
                    if let Some(ver_peer) = dep_ref.ver_peer() {
                        let version_str = ver_peer.version().to_string();
                        if !satisfies(&version_str, peer_range) {
                            issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                                parents: current_parents.clone(),
                                optional: is_optional,
                                wanted_range: peer_range.clone(),
                                found_version: version_str,
                                resolved_from: Vec::new(),
                            });
                        }
                    } else if let Some(link_target) = dep_ref.as_link_target() {
                        issues.bad.entry(peer_name.clone()).or_default().push(BadPeerIssue {
                            parents: current_parents.clone(),
                            optional: is_optional,
                            wanted_range: peer_range.clone(),
                            found_version: format!("link:{link_target}"),
                            resolved_from: Vec::new(),
                        });
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

    let all_deps = snapshot
        .dependencies
        .iter()
        .flat_map(|d| d.iter())
        .chain(snapshot.optional_dependencies.iter().flat_map(|d| d.iter()));

    for (alias, dep_ref) in all_deps {
        if let Some(child_key) = dep_ref.resolve(alias) {
            walk_snapshot(&child_key, snapshots, packages, &current_parents, issues, visited);
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

fn merge_missing_peers(missing: &HashMap<String, Vec<MissingPeerIssue>>) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut intersections = HashMap::new();

    for (peer_name, issues) in missing {
        if issues.iter().all(|i| i.optional) {
            continue;
        }
        if issues.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let ranges: Vec<&str> = issues.iter().map(|i| i.wanted_range.as_str()).collect();
        let unique: HashSet<&&str> = ranges.iter().collect();
        if unique.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let range_owned: Vec<String> = issues.iter().map(|i| i.wanted_range.clone()).collect();
        if have_common_version(&range_owned) {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
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

fn have_common_version(ranges: &[String]) -> bool {
    if ranges.len() <= 1 {
        return true;
    }

    let candidate_versions: Vec<String> =
        ranges.iter().filter_map(|r| extract_candidate_version(r)).collect();

    if candidate_versions.is_empty() {
        return false;
    }

    for ver_str in &candidate_versions {
        if ranges.iter().all(|r| satisfies(ver_str, r)) {
            return true;
        }
    }

    false
}

fn extract_candidate_version(range_str: &str) -> Option<String> {
    for segment in range_str.split("||") {
        let segment = segment.trim();
        let cleaned = segment
            .strip_prefix(">=")
            .or_else(|| segment.strip_prefix('>'))
            .or_else(|| segment.strip_prefix("<="))
            .or_else(|| segment.strip_prefix('<'))
            .or_else(|| segment.strip_prefix('^'))
            .or_else(|| segment.strip_prefix('~'))
            .unwrap_or(segment)
            .trim();

        let cleaned =
            if let Some((start, _)) = cleaned.split_once(" - ") { start.trim() } else { cleaned };

        let first_word = cleaned.split_whitespace().next().unwrap_or(cleaned);
        let normalized = first_word.replace(['x', 'X'], "0");

        if normalized != "*" && !normalized.is_empty() && Version::parse(&normalized).is_ok() {
            return Some(normalized);
        }
    }
    None
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

    for project_issues in issues.values_mut() {
        let mut filtered_missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
        let mut filtered_bad: HashMap<String, Vec<BadPeerIssue>> = HashMap::new();
        let mut filtered_intersections: HashMap<String, String> = HashMap::new();

        for (peer_name, peer_issues) in &project_issues.missing {
            if ignore_missing_pats.iter().any(|pat| simple_glob_match(pat, peer_name))
                || peer_issues.iter().all(|i| i.optional)
            {
                continue;
            }
            filtered_missing.insert(peer_name.clone(), peer_issues.clone());
            if let Some(range) = project_issues.intersections.get(peer_name) {
                filtered_intersections.insert(peer_name.clone(), range.clone());
            }
        }

        for (peer_name, peer_issues) in &project_issues.bad {
            if allow_any_pats.iter().any(|pat| simple_glob_match(pat, peer_name)) {
                continue;
            }
            let remaining: Vec<BadPeerIssue> = peer_issues
                .iter()
                .filter(|issue| {
                    if let Some(ranges) = allow_all_matcher.get(peer_name)
                        && ranges.iter().any(|r| satisfies(&issue.found_version, r))
                    {
                        return false;
                    }
                    if let Some(declaring_parent) = issue.parents.last()
                        && let Some(parent_map) = allow_by_parent.get(&declaring_parent.name)
                        && let Some(ranges) = parent_map.get(peer_name)
                        && ranges.iter().any(|r| satisfies(&issue.found_version, r))
                    {
                        return false;
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
        project_issues.intersections = filtered_intersections;
    }

    issues
}

type AllowAllMatcher = HashMap<String, Vec<String>>;
type AllowByParentMatcher = HashMap<String, HashMap<String, Vec<String>>>;

fn parse_allowed_versions(
    allowed: &HashMap<String, String>,
) -> (AllowAllMatcher, AllowByParentMatcher) {
    let mut match_all: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_parent: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();

    for (selector, spec) in allowed {
        if let Some((parent, target)) = selector.split_once('>') {
            let parent_name = parent.trim().to_string();
            let target_name = target.trim().to_string();
            let ranges: Vec<String> = spec.split("||").map(|s| s.trim().to_string()).collect();
            by_parent
                .entry(parent_name)
                .or_default()
                .entry(target_name)
                .or_default()
                .extend(ranges);
        } else {
            let target_name = if let Some((name, _version)) = selector.split_once('@') {
                name.to_string()
            } else {
                selector.clone()
            };
            let ranges: Vec<String> = spec.split("||").map(|s| s.trim().to_string()).collect();
            match_all.entry(target_name).or_default().extend(ranges);
        }
    }

    (match_all, by_parent)
}

fn simple_glob_match(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }
    let segments: Vec<&str> = pattern.split('*').collect();
    let mut rest = value;
    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        if i == 0 {
            let Some(stripped) = rest.strip_prefix(segment) else { return false };
            rest = stripped;
        } else if i == segments.len() - 1 {
            return rest.ends_with(segment);
        } else if let Some(pos) = rest.find(segment) {
            rest = &rest[pos + segment.len()..];
        } else {
            return false;
        }
    }
    true
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
