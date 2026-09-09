//! `pnpm list` output renderers (tree / parseable / JSON), mirroring
//! the TypeScript `@pnpm/deps.inspection.list` renderers byte for byte.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use serde_json::{Map, Value, json};

use crate::cli_args::deps_tree::{
    DependencyNode,
    build::DependenciesHierarchy,
    render::{
        ColorFn, LongPkgInfo, PeerVariants, TreeNode, TreeNodeGroup, blue, bold_styled,
        cyan_bright, deduped_label, dim, gray, name_at_version, peer_hash_suffix, plain,
        read_long_pkg_info, red, render_archy, yellow,
    },
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

// --- parseable ---------------------------------------------------------------

pub(crate) struct RenderParseableOptions {
    pub long: bool,
    pub always_print_root_package: bool,
}

pub(crate) fn render_parseable(
    projects: &[ProjectHierarchy],
    opts: &RenderParseableOptions,
) -> String {
    let mut dep_paths: HashSet<String> = HashSet::new();
    projects
        .iter()
        .map(|project| render_parseable_for_project(&mut dep_paths, project, opts))
        .filter(|out| !out.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_parseable_for_project(
    dep_paths: &mut HashSet<String>,
    project: &ProjectHierarchy,
    opts: &RenderParseableOptions,
) -> String {
    let root_already_seen = dep_paths.contains(&project.path);
    dep_paths.insert(project.path.clone());
    let all_deps: Vec<&DependencyNode> = project
        .hierarchy
        .optional_dependencies
        .iter()
        .chain(&project.hierarchy.dependencies)
        .chain(&project.hierarchy.dev_dependencies)
        .chain(&project.hierarchy.unsaved_dependencies)
        .collect();
    let mut flattened = flatten(dep_paths, &all_deps);
    flattened.sort_by(|a, b| a.name.cmp(&b.name));
    if root_already_seen && flattened.is_empty() {
        return String::new();
    }
    if !opts.always_print_root_package && flattened.is_empty() && all_deps.is_empty() {
        return String::new();
    }

    let mut lines: Vec<String> = Vec::new();
    if !root_already_seen {
        lines.push(parseable_project_line(project, opts.long));
    }
    lines.extend(flattened.into_iter().map(|node| parseable_node_line(node, opts.long)));
    lines.join("\n")
}

/// The project's own parseable line: its path, and under `--long` the
/// name, version and privacy the manifest declares.
fn parseable_project_line(project: &ProjectHierarchy, long: bool) -> String {
    let mut line = plain(&project.path);
    let Some(name) = project.name.as_ref().filter(|_| long) else {
        return line;
    };
    line.push(':');
    line.push_str(&plain(name));
    if let Some(version) = &project.version {
        line.push('@');
        line.push_str(&plain(version));
    }
    if project.private {
        line.push_str(":PRIVATE");
    }
    line
}

/// One package's parseable line: its path, and under `--long` the
/// specifier it resolves as — including the `npm:` alias form.
fn parseable_node_line(node: &DependencyNode, long: bool) -> String {
    if !long {
        return plain(&node.path);
    }
    let path = plain(&node.path);
    let alias = plain(&node.alias);
    let name = plain(&node.name);
    let version = plain(&node.version);
    if alias == name {
        return if version.contains('@') {
            format!("{path}:{version}")
        } else {
            format!("{path}:{name}@{version}")
        };
    }
    if version.contains('@') {
        format!("{path}:{alias} {version}")
    } else {
        format!("{path}:{alias} npm:{name}@{version}")
    }
}

fn flatten<'a>(
    dep_paths: &mut HashSet<String>,
    nodes: &[&'a DependencyNode],
) -> Vec<&'a DependencyNode> {
    let mut packages: Vec<&'a DependencyNode> = Vec::new();
    for node in nodes {
        // Parseable output is flat, so packages that several parents
        // depend on are printed once.
        if !dep_paths.contains(&node.path) {
            dep_paths.insert(node.path.clone());
            packages.push(node);
        }
        if !node.dependencies.is_empty() {
            let children: Vec<&DependencyNode> = node.dependencies.iter().collect();
            packages.extend(flatten(dep_paths, &children));
        }
    }
    packages
}

// --- JSON --------------------------------------------------------------------

pub(crate) fn render_json(projects: &[ProjectHierarchy], long: bool) -> String {
    let arr: Vec<Value> = projects
        .iter()
        .map(|project| {
            let mut obj = Map::new();
            if let Some(name) = &project.name {
                obj.insert("name".to_string(), json!(name));
            }
            if let Some(version) = &project.version {
                obj.insert("version".to_string(), json!(version));
            }
            obj.insert("path".to_string(), json!(project.path));
            obj.insert("private".to_string(), json!(project.private));
            let fields: [(&str, &Vec<DependencyNode>); 4] = [
                ("dependencies", &project.hierarchy.dependencies),
                ("devDependencies", &project.hierarchy.dev_dependencies),
                ("optionalDependencies", &project.hierarchy.optional_dependencies),
                ("unsavedDependencies", &project.hierarchy.unsaved_dependencies),
            ];
            for (field, nodes) in fields {
                if !nodes.is_empty() {
                    obj.insert(field.to_string(), Value::Object(to_json_result(nodes, long)));
                }
            }
            Value::Object(obj)
        })
        .collect();
    serde_json::to_string_pretty(&arr).expect("serialize list JSON")
}

fn to_json_result(nodes: &[DependencyNode], long: bool) -> Map<String, Value> {
    let mut sorted: Vec<&DependencyNode> = nodes.iter().collect();
    sorted.sort_by(|left, right| left.alias.cmp(&right.alias));
    sorted
        .into_iter()
        .map(|node| (node.alias.clone(), Value::Object(node_to_json(node, long))))
        .collect()
}

/// One package's JSON object, with its own dependencies nested under it.
fn node_to_json(node: &DependencyNode, long: bool) -> Map<String, Value> {
    let mut dep = Map::new();
    dep.insert("from".to_string(), json!(node.name));
    dep.insert("version".to_string(), json!(node.version));
    if let Some(resolved) = &node.resolved {
        dep.insert("resolved".to_string(), json!(resolved));
    }
    if long {
        insert_long_pkg_info(&mut dep, &read_long_pkg_info(Path::new(&node.path)));
    }
    dep.insert("path".to_string(), json!(node.path));
    let sub_dependencies = to_json_result(&node.dependencies, long);
    if !sub_dependencies.is_empty() {
        dep.insert("dependencies".to_string(), Value::Object(sub_dependencies));
    }
    if node.deduped {
        dep.insert("deduped".to_string(), json!(true));
        if let Some(count) = node.deduped_dependencies_count {
            dep.insert("dedupedDependenciesCount".to_string(), json!(count));
        }
    }
    dep
}

/// The `--long` manifest fields, each present only when the package
/// declares it.
fn insert_long_pkg_info(dep: &mut Map<String, Value>, info: &LongPkgInfo) {
    if let Some(description) = &info.description {
        dep.insert("description".to_string(), json!(description));
    }
    if let Some(license) = &info.license {
        dep.insert("license".to_string(), license.clone());
    }
    if let Some(author) = &info.author {
        dep.insert("author".to_string(), author.clone());
    }
    if let Some(homepage) = &info.homepage {
        dep.insert("homepage".to_string(), json!(homepage));
    }
    if let Some(repository) = &info.repository {
        dep.insert("repository".to_string(), json!(repository));
    }
}
