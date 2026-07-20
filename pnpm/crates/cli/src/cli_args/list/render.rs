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
        ColorFn, LongPkgInfo, PeerVariants, TreeNode, TreeNodeGroup, blue, bold, cyan_bright,
        deduped_label, dim, gray, name_at_version, peer_hash_suffix, plain, read_long_pkg_info,
        red, render_archy, yellow,
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

    let mut label = String::new();
    if let Some(name) = &project.name {
        label.push_str(&name_at_version(name, project.version.as_deref().unwrap_or(""), plain));
        label.push(' ');
    }
    label.push_str(&dim(&project.path));
    if project.private {
        label.push_str(&dim(" (PRIVATE)"));
    }

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

    let root_label = bold(&label);
    if groups.is_empty() {
        return Some(root_label);
    }
    let tree = TreeNode { label: root_label, groups };
    Some(render_archy(&tree).trim_end().to_string())
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
            let mut label_lines = vec![print_label(get_color, Some(multi_peer_pkgs), node)];
            if let Some(message) = &node.search_message {
                label_lines.push(plain(message));
            }
            if long {
                let info = read_long_pkg_info(Path::new(&node.path));
                if let Some(description) = info.description {
                    label_lines.push(plain(&description));
                }
                if let Some(repository) = info.repository {
                    label_lines.push(plain(&repository));
                }
                if let Some(homepage) = info.homepage {
                    label_lines.push(plain(&homepage));
                }
                if !node.path.is_empty() {
                    label_lines.push(plain(&node.path));
                }
            }
            TreeNode::with_children(label_lines.join("\n"), children)
        })
        .collect()
}

fn print_label(
    get_color: PkgColor,
    multi_peer_pkgs: Option<&HashMap<String, usize>>,
    node: &DependencyNode,
) -> String {
    let color = get_color(node);
    let mut label = if node.alias == node.name {
        name_at_version(&node.name, &node.version, color)
    } else {
        // An npm: protocol alias displays as `alias@npm:name@version`,
        // unless the version already carries an `@` (file:, link:, ...).
        if node.version.contains('@') {
            format!("{}{}", color(&node.alias), gray(&format!("@{}", node.version)))
        } else {
            format!(
                "{}{}",
                color(&node.alias),
                gray(&format!("@npm:{}@{}", node.name, node.version)),
            )
        }
    };
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
    if node.searched { bold(&label) } else { label }
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
        let mut first_line = plain(&project.path);
        if opts.long
            && let Some(name) = &project.name
        {
            first_line.push(':');
            first_line.push_str(&plain(name));
            if let Some(version) = &project.version {
                first_line.push('@');
                first_line.push_str(&plain(version));
            }
            if project.private {
                first_line.push_str(":PRIVATE");
            }
        }
        lines.push(first_line);
    }
    for node in flattened {
        if opts.long {
            let path = plain(&node.path);
            let alias = plain(&node.alias);
            let name = plain(&node.name);
            let version = plain(&node.version);
            if alias != name {
                if version.contains('@') {
                    lines.push(format!("{path}:{alias} {version}"));
                } else {
                    lines.push(format!("{path}:{alias} npm:{name}@{version}"));
                }
            } else if version.contains('@') {
                lines.push(format!("{path}:{version}"));
            } else {
                lines.push(format!("{path}:{name}@{version}"));
            }
        } else {
            lines.push(plain(&node.path));
        }
    }
    lines.join("\n")
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
    sorted.sort_by(|a, b| a.alias.cmp(&b.alias));
    let mut result = Map::new();
    for node in sorted {
        let sub_dependencies = to_json_result(&node.dependencies, long);
        let mut dep = Map::new();
        dep.insert("from".to_string(), json!(node.name));
        dep.insert("version".to_string(), json!(node.version));
        if let Some(resolved) = &node.resolved {
            dep.insert("resolved".to_string(), json!(resolved));
        }
        if long {
            let info: LongPkgInfo = read_long_pkg_info(Path::new(&node.path));
            if let Some(description) = info.description {
                dep.insert("description".to_string(), json!(description));
            }
            if let Some(license) = info.license {
                dep.insert("license".to_string(), license);
            }
            if let Some(author) = info.author {
                dep.insert("author".to_string(), author);
            }
            if let Some(homepage) = info.homepage {
                dep.insert("homepage".to_string(), json!(homepage));
            }
            if let Some(repository) = info.repository {
                dep.insert("repository".to_string(), json!(repository));
            }
        }
        dep.insert("path".to_string(), json!(node.path));
        if !sub_dependencies.is_empty() {
            dep.insert("dependencies".to_string(), Value::Object(sub_dependencies));
        }
        if node.deduped {
            dep.insert("deduped".to_string(), json!(true));
            if let Some(count) = node.deduped_dependencies_count {
                dep.insert("dedupedDependenciesCount".to_string(), json!(count));
            }
        }
        result.insert(node.alias.clone(), Value::Object(dep));
    }
    result
}
