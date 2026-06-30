//! `pacquet why` — show the packages that depend on `<pkg>`.
//!
//! Ports pnpm's
//! [`why` command](https://github.com/pnpm/pnpm/blob/deps/inspection/commands/src/listing/why.ts)
//! and the reverse-tree builder in
//! [`buildDependentsTree`](https://github.com/pnpm/pnpm/blob/deps/inspection/tree-builder/src/buildDependentsTree.ts).
//!

use crate::{State, cli_args::sanitize::sanitize};
use clap::Args;
use owo_colors::{OwoColorize, Stream};
use pacquet_config::matcher::{Matcher, create_matcher};
use pacquet_lockfile::{Lockfile, PackageKey, PackageMetadata, PkgNameVerPeer};
use pacquet_package_manifest::DependencyGroup;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::Write,
};

#[derive(Debug, Clone)]
struct DependentNode {
    name: String,
    version: String,
    dep_field: Option<DependencyGroup>,
    dependents: Vec<DependentNode>,
}

#[derive(Debug)]
struct WhyResult {
    name: String,
    version: String,
    dependents: Vec<DependentNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ParentNode {
    Package(PkgNameVerPeer),
    Importer(String),
}

impl fmt::Display for ParentNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParentNode::Package(key) => write!(f, "{key}"),
            ParentNode::Importer(id) => write!(f, "{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ReverseKey {
    Package(PkgNameVerPeer),
    Importer(String),
}

#[derive(Debug)]
struct ImporterInfo {
    name: String,
    version: String,
}

struct WalkCtx<'a> {
    reverse_map: &'a HashMap<ReverseKey, Vec<(ParentNode, String)>>,
    importer_info: &'a HashMap<String, ImporterInfo>,
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
}

#[derive(Debug, Args)]
pub struct WhyArgs {
    pub packages: Vec<String>,

    #[clap(long)]
    pub depth: Option<usize>,
}

impl WhyArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        if self.packages.is_empty() {
            return Err(miette::miette!(
                code = "ERR_PNPM_MISSING_PACKAGE_NAME",
                "`pacquet why` requires a package name or pattern"
            ));
        }

        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        let Some(lockfile) = lockfile else {
            return Ok(());
        };

        let matcher = create_matcher(&self.packages);

        let manifest_value = state.manifest.value();
        let root_name = manifest_value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("the root project")
            .to_string();
        let root_version =
            manifest_value.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let mut importer_info = HashMap::new();
        for importer_id in lockfile.importers.keys() {
            if importer_id == Lockfile::ROOT_IMPORTER_KEY {
                importer_info.insert(
                    importer_id.clone(),
                    ImporterInfo { name: root_name.clone(), version: root_version.clone() },
                );
            } else {
                importer_info.insert(
                    importer_id.clone(),
                    ImporterInfo { name: importer_id.clone(), version: String::new() },
                );
            }
        }

        let results = build_dependents_tree(lockfile, &matcher, &importer_info, self.depth);

        if results.is_empty() {
            return Ok(());
        }

        let output = render_tree(&results, self.depth);
        let mut stdout = std::io::stdout();
        let _ = write!(stdout, "{output}");
        let _ = stdout.flush();

        Ok(())
    }
}

const MAX_REVERSE_WALK_DEPTH: usize = 64;

fn display_version(
    key: &PkgNameVerPeer,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> String {
    let metadata_key = key.without_peer();
    packages
        .and_then(|pkg_map| pkg_map.get(&metadata_key))
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| key.suffix.version().to_string())
}

fn resolve_link_to_importer(
    parent_importer_id: &str,
    link_target: &str,
    importer_ids: &HashMap<String, impl std::any::Any>,
) -> Option<String> {
    let resolved = normalize_path(parent_importer_id, link_target)?;
    importer_ids.contains_key(&resolved).then_some(resolved)
}

fn normalize_path(base: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').filter(|segment| !segment.is_empty()) {
        parts.push(part);
    }
    for part in relative.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn build_dependents_tree(
    lockfile: &Lockfile,
    matcher: &Matcher,
    importer_info: &HashMap<String, ImporterInfo>,
    max_depth: Option<usize>,
) -> Vec<WhyResult> {
    let packages = lockfile.packages.as_ref();
    let snapshots = lockfile.snapshots.as_ref();

    let mut forward_edges: HashMap<PkgNameVerPeer, Vec<(String, Option<PkgNameVerPeer>)>> =
        HashMap::new();

    let node_keys: Vec<PackageKey> = if let Some(pkgs) = packages {
        pkgs.keys().cloned().collect()
    } else if let Some(snapshot_map) = snapshots {
        snapshot_map.keys().cloned().collect()
    } else {
        return vec![];
    };

    for key in &node_keys {
        let Some(snapshot) = snapshots.and_then(|s| s.get(key)) else {
            continue;
        };

        let mut edges: Vec<(String, Option<PkgNameVerPeer>)> = Vec::new();

        if let Some(deps) = &snapshot.dependencies {
            for (alias, dep_ref) in deps {
                edges.push((alias.to_string(), dep_ref.resolve(alias)));
            }
        }

        if let Some(optional_deps) = &snapshot.optional_dependencies {
            for (alias, dep_ref) in optional_deps {
                edges.push((alias.to_string(), dep_ref.resolve(alias)));
            }
        }

        forward_edges.insert(key.clone(), edges);
    }

    let mut reverse_map: HashMap<ReverseKey, Vec<(ParentNode, String)>> = HashMap::new();

    let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    for importer_id in importer_info.keys() {
        let Some(importer) = lockfile.importers.get(importer_id.as_str()) else {
            continue;
        };
        for group in groups {
            if let Some(deps) = importer.get_map_by_group(group) {
                for (alias, spec) in deps {
                    if let Some(target_key) = spec.version.resolved_key(alias) {
                        reverse_map
                            .entry(ReverseKey::Package(target_key))
                            .or_default()
                            .push((ParentNode::Importer(importer_id.clone()), alias.to_string()));
                    } else if let Some(link_target) = spec.version.as_link_target()
                        && let Some(resolved_id) =
                            resolve_link_to_importer(importer_id, link_target, &lockfile.importers)
                    {
                        reverse_map
                            .entry(ReverseKey::Importer(resolved_id))
                            .or_default()
                            .push((ParentNode::Importer(importer_id.clone()), alias.to_string()));
                    }
                }
            }
        }
    }

    for (parent_key, edges) in &forward_edges {
        for (alias, target) in edges {
            if let Some(target_key) = target {
                reverse_map
                    .entry(ReverseKey::Package(target_key.clone()))
                    .or_default()
                    .push((ParentNode::Package(parent_key.clone()), alias.clone()));
            }
        }
    }

    for edges in reverse_map.values_mut() {
        edges.sort_by_cached_key(|entry| entry.0.to_string());
    }

    let ctx = WalkCtx { reverse_map: &reverse_map, importer_info, packages };

    let mut matched_roots: Vec<(String, String, ReverseKey)> = Vec::new();

    for key in &node_keys {
        let name = key.name.to_string();
        let version = display_version(key, packages);

        let matched = matcher.matches(&name)
            || reverse_map
                .get(&ReverseKey::Package(key.clone()))
                .is_some_and(|edges| edges.iter().any(|(_parent, alias)| matcher.matches(alias)));

        if matched {
            matched_roots.push((name, version, ReverseKey::Package(key.clone())));
        }
    }

    for importer_id in importer_info.keys() {
        let info = importer_info.get(importer_id.as_str());
        let name = info.map_or_else(|| importer_id.clone(), |i| i.name.clone());

        let matched = matcher.matches(&name)
            || reverse_map
                .get(&ReverseKey::Importer(importer_id.clone()))
                .is_some_and(|edges| edges.iter().any(|(_parent, alias)| matcher.matches(alias)));

        if matched {
            let version = info.map_or_else(String::new, |i| i.version.clone());
            matched_roots.push((name, version, ReverseKey::Importer(importer_id.clone())));
        }
    }

    let mut memo: HashMap<(ReverseKey, usize), Vec<DependentNode>> = HashMap::new();
    let mut results: Vec<WhyResult> = Vec::new();

    for (name, version, root_key) in &matched_roots {
        let mut visited = HashSet::new();
        match root_key {
            ReverseKey::Package(key) => {
                visited.insert(ParentNode::Package(key.clone()));
            }
            ReverseKey::Importer(id) => {
                visited.insert(ParentNode::Importer(id.clone()));
            }
        }
        let mut expanded = HashSet::new();
        let dependents =
            walk_reverse(root_key, &ctx, &mut visited, &mut expanded, &mut memo, 0, max_depth);

        results.push(WhyResult { name: name.clone(), version: version.clone(), dependents });
    }

    results
        .sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));

    results
}

fn walk_reverse(
    node_key: &ReverseKey,
    ctx: &WalkCtx<'_>,
    visited: &mut HashSet<ParentNode>,
    expanded: &mut HashSet<ParentNode>,
    memo: &mut HashMap<(ReverseKey, usize), Vec<DependentNode>>,
    depth: usize,
    max_depth: Option<usize>,
) -> Vec<DependentNode> {
    if depth >= MAX_REVERSE_WALK_DEPTH || max_depth.is_some_and(|max| depth >= max) {
        return vec![];
    }

    let Some(edges) = ctx.reverse_map.get(node_key) else {
        return vec![];
    };

    let memo_key = (node_key.clone(), depth);
    if let Some(cached) = memo.get(&memo_key) {
        return cached.clone();
    }

    let mut dependents = Vec::new();

    for (parent_node, _alias) in edges {
        match parent_node {
            ParentNode::Importer(importer_id) => {
                let info = ctx.importer_info.get(importer_id.as_str());
                dependents.push(DependentNode {
                    name: info.map_or_else(|| importer_id.clone(), |i| i.name.clone()),
                    version: info.map_or_else(String::new, |i| i.version.clone()),
                    dep_field: None,
                    dependents: vec![],
                });
            }
            ParentNode::Package(parent_key) => {
                if visited.contains(parent_node) {
                    dependents.push(DependentNode {
                        name: parent_key.name.to_string(),
                        version: display_version(parent_key, ctx.packages),
                        dep_field: None,
                        dependents: vec![],
                    });
                    continue;
                }

                if expanded.contains(parent_node) {
                    dependents.push(DependentNode {
                        name: parent_key.name.to_string(),
                        version: display_version(parent_key, ctx.packages),
                        dep_field: None,
                        dependents: vec![],
                    });
                    continue;
                }

                visited.insert(parent_node.clone());
                expanded.insert(parent_node.clone());

                let parent_name = parent_key.name.to_string();
                let parent_version = display_version(parent_key, ctx.packages);

                let child_dependents = walk_reverse(
                    &ReverseKey::Package(parent_key.clone()),
                    ctx,
                    visited,
                    expanded,
                    memo,
                    depth + 1,
                    max_depth,
                );

                visited.remove(parent_node);

                dependents.push(DependentNode {
                    name: parent_name,
                    version: parent_version,
                    dep_field: None,
                    dependents: child_dependents,
                });
            }
        }
    }

    dependents
        .sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));

    memo.insert(memo_key, dependents.clone());
    dependents
}

fn render_tree(results: &[WhyResult], max_depth: Option<usize>) -> String {
    let mut output = String::new();

    for (i, result) in results.iter().enumerate() {
        if i > 0 {
            output.push_str("\n\n");
        }

        let root_label = format!("{}{}", bold(&result.name), dim(&format!("@{}", result.version)));
        output.push_str(&root_label);

        if result.dependents.is_empty() {
            continue;
        }

        output.push('\n');
        render_dependents(&mut output, &result.dependents, "", max_depth, 0);
    }

    output
}

fn render_dependents(
    output: &mut String,
    dependents: &[DependentNode],
    prefix: &str,
    max_depth: Option<usize>,
    current_depth: usize,
) {
    if let Some(max) = max_depth
        && current_depth >= max
    {
        return;
    }

    for (i, dep) in dependents.iter().enumerate() {
        let is_last = i == dependents.len() - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let label = format!(
            "{}{}{}",
            bold(&dep.name),
            dim(&format!("@{}", dep.version)),
            dep.dep_field
                .map(|field| format!(" {}", dim(&format!("({})", dep_field_name(field)))))
                .unwrap_or_default(),
        );

        output.push_str(prefix);
        output.push_str(connector);
        output.push_str(&label);
        output.push('\n');

        if !dep.dependents.is_empty() {
            let new_prefix = format!("{prefix}{child_prefix}");
            render_dependents(output, &dep.dependents, &new_prefix, max_depth, current_depth + 1);
        }
    }
}

fn dep_field_name(field: DependencyGroup) -> &'static str {
    match field {
        DependencyGroup::Prod => "dependencies",
        DependencyGroup::Dev => "devDependencies",
        DependencyGroup::Optional => "optionalDependencies",
        DependencyGroup::Peer => "peerDependencies",
    }
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dim(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

#[cfg(test)]
mod tests;
