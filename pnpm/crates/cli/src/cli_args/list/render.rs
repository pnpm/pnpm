//! `pnpm list` output renderers (tree / parseable / JSON), mirroring
//! the TypeScript `@pnpm/deps.inspection.list` renderers byte for byte.

pub(crate) use structured::{RenderParseableOptions, render_json, render_parseable};

use crate::cli_args::deps_tree::{
    DependencyNode,
    build::DependenciesHierarchy,
    render::{
        ColorFn, LongPkgInfo, PeerVariants, TreeNode, TreeNodeGroup, blue, bold_styled,
        cyan_bright, deduped_label, dim, gray, name_at_version, peer_hash_suffix, plain,
        read_long_pkg_info, red, render_archy, yellow,
    },
};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// One project (importer) with its categorized dependency hierarchy —
/// the unit the renderers consume.
#[derive(Debug)]
pub(crate) struct ProjectHierarchy {
    pub name: Option<String>,
    pub version: Option<String>,
    pub private: bool,
    pub path: String,
    pub hierarchy: DependenciesHierarchy,
}

impl ProjectHierarchy {
    fn groups(&self) -> [(&'static str, &Vec<DependencyNode>); 3] {
        [
            ("dependencies", &self.hierarchy.dependencies),
            ("devDependencies", &self.hierarchy.dev_dependencies),
            ("optionalDependencies", &self.hierarchy.optional_dependencies),
        ]
    }
}

pub(crate) struct RenderTreeOptions {
    pub always_print_root_package: bool,
    /// `false` when `--depth -1` printed project roots only (which
    /// suppresses the legend and summary).
    pub depth_above_projects_only: bool,
    pub long: bool,
    pub show_extraneous: bool,
    pub show_summary: bool,
}

fn legend() -> String {
    format!(
        "Legend: {}, {}, {}\n\n",
        plain("production dependency"),
        blue("optional only"),
        yellow("dev only"),
    )
}

pub(crate) fn render_tree(projects: &[ProjectHierarchy], opts: &RenderTreeOptions) -> String {
    let multi_peer_pkgs = find_multi_peer_packages(projects);
    let output = projects
        .iter()
        .filter_map(|project| render_tree_for_project(project, opts, &multi_peer_pkgs))
        .collect::<Vec<_>>()
        .join("\n\n");
    let legend =
        if opts.depth_above_projects_only && !output.is_empty() { legend() } else { String::new() };
    let summary = if opts.show_summary && opts.depth_above_projects_only && !output.is_empty() {
        format!("\n\n{}", list_summary(projects))
    } else {
        String::new()
    };
    format!("{legend}{output}{summary}")
}

fn render_tree_for_project(
    project: &ProjectHierarchy,
    opts: &RenderTreeOptions,
    multi_peer_pkgs: &HashMap<String, usize>,
) -> Option<String> {
    let has_deps = project.groups().iter().any(|(_, nodes)| !nodes.is_empty())
        || (opts.show_extraneous && !project.hierarchy.unsaved_dependencies.is_empty());
    if !opts.always_print_root_package && !has_deps {
        return None;
    }

    let label = project_label(project);
    let mut groups: Vec<TreeNodeGroup> = Vec::new();
    for (field, nodes) in project.groups() {
        if nodes.is_empty() {
            continue;
        }
        groups.push(TreeNodeGroup {
            group: cyan_bright(&format!("{field}:")),
            nodes: to_archy_nodes(get_pkg_color, nodes, opts.long, multi_peer_pkgs),
        });
    }
    if opts.show_extraneous && !project.hierarchy.unsaved_dependencies.is_empty() {
        groups.push(TreeNodeGroup {
            group: cyan_bright(
                "not saved (you should add these dependencies to package.json if you need them):",
            ),
            nodes: to_archy_nodes(
                unsaved_color,
                &project.hierarchy.unsaved_dependencies,
                opts.long,
                multi_peer_pkgs,
            ),
        });
    }

    let root_label = bold_styled(&label);
    if groups.is_empty() {
        return Some(root_label);
    }
    let tree = TreeNode { label: root_label, groups };
    Some(render_archy(&tree).trim_end().to_string())
}

/// The project's own line: its name and version when it has them, then
/// its path.
fn project_label(project: &ProjectHierarchy) -> String {
    let mut label = String::new();
    if let Some(name) = &project.name {
        label.push_str(&name_at_version(name, project.version.as_deref().unwrap_or(""), plain));
        label.push(' ');
    }
    label.push_str(&dim(&project.path));
    if project.private {
        label.push_str(&dim(" (PRIVATE)"));
    }
    label
}

type PkgColor = fn(&DependencyNode) -> ColorFn;

fn get_pkg_color(node: &DependencyNode) -> ColorFn {
    if node.dev == Some(true) {
        yellow
    } else if node.optional {
        blue
    } else {
        plain
    }
}

fn unsaved_color(_node: &DependencyNode) -> ColorFn {
    red
}

fn to_archy_nodes(
    get_color: PkgColor,
    nodes: &[DependencyNode],
    long: bool,
    multi_peer_pkgs: &HashMap<String, usize>,
) -> Vec<TreeNode> {
    let mut sorted: Vec<&DependencyNode> = nodes.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
        .iter()
        .map(|node| {
            let children = if node.deduped {
                Vec::new()
            } else {
                to_archy_nodes(get_color, &node.dependencies, long, multi_peer_pkgs)
            };
            let label = node_label_lines(get_color, multi_peer_pkgs, node, long);
            TreeNode::with_children(label, children)
        })
        .collect()
}

/// One node's label, plus the search message and the `--long` manifest
/// fields, one per line.
fn node_label_lines(
    get_color: PkgColor,
    multi_peer_pkgs: &HashMap<String, usize>,
    node: &DependencyNode,
    long: bool,
) -> String {
    let mut label_lines = vec![print_label(get_color, Some(multi_peer_pkgs), node)];
    if let Some(message) = &node.search_message {
        label_lines.push(plain(message));
    }
    if long {
        let info = read_long_pkg_info(Path::new(&node.path));
        label_lines.extend(
            [info.description, info.repository, info.homepage]
                .into_iter()
                .flatten()
                .map(|line| plain(&line)),
        );
        if !node.path.is_empty() {
            label_lines.push(plain(&node.path));
        }
    }
    label_lines.join("\n")
}

fn print_label(
    get_color: PkgColor,
    multi_peer_pkgs: Option<&HashMap<String, usize>>,
    node: &DependencyNode,
) -> String {
    let mut label = node_name_label(get_color(node), node);
    if node.is_peer {
        label.push_str(" peer");
    }
    if node.is_skipped {
        label.push_str(" skipped");
    }
    if let Some(multi_peer_pkgs) = multi_peer_pkgs {
        label.push_str(&peer_hash_suffix(
            multi_peer_pkgs,
            &node.name,
            &node.version,
            node.peers_suffix_hash.as_deref(),
        ));
    }
    if node.deduped {
        label.push_str(&deduped_label());
    }
    if node.searched { bold_styled(&label) } else { label }
}

/// `name@version`, or — for an npm: protocol alias —
/// `alias@npm:name@version`, unless the version already carries an `@`
/// (`file:`, `link:`, ...).
fn node_name_label(color: ColorFn, node: &DependencyNode) -> String {
    if node.alias == node.name {
        return name_at_version(&node.name, &node.version, color);
    }
    if node.version.contains('@') {
        return format!("{}{}", color(&node.alias), gray(&format!("@{}", node.version)));
    }
    format!("{}{}", color(&node.alias), gray(&format!("@npm:{}@{}", node.name, node.version)))
}

fn find_multi_peer_packages(projects: &[ProjectHierarchy]) -> HashMap<String, usize> {
    let mut variants = PeerVariants::default();
    fn walk(variants: &mut PeerVariants, nodes: &[DependencyNode]) {
        for node in nodes {
            variants.collect(&node.name, &node.version, node.peers_suffix_hash.as_deref());
            walk(variants, &node.dependencies);
        }
    }
    for project in projects {
        for (_, nodes) in project.groups() {
            walk(&mut variants, nodes);
        }
    }
    variants.into_multi_variant_counts()
}

fn list_summary(projects: &[ProjectHierarchy]) -> String {
    fn count(nodes: &[DependencyNode]) -> u64 {
        nodes.iter().map(|node| 1 + count(&node.dependencies)).sum()
    }
    let total: u64 = projects
        .iter()
        .map(|project| project.groups().iter().map(|(_, nodes)| count(nodes)).sum::<u64>())
        .sum();
    let mut parts = vec![format!("{total} package{}", if total == 1 { "" } else { "s" })];
    if projects.len() > 1 {
        parts.push(format!("{} projects", projects.len()));
    }
    dim(&parts.join(" in "))
}

mod structured;
