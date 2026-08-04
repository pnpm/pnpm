//! `pnpm why` output renderers (tree / parseable / JSON), mirroring the
//! TypeScript `renderDependentsTree` / `renderDependentsJson` /
//! `renderDependentsParseable`.

use std::{collections::HashMap, path::Path};

use crate::cli_args::deps_tree::{
    dependents::{DependentNode, DependentsTree},
    render::{
        PeerVariants, TreeNode, bold, circular_label, deduped_label, dim, name_at_version,
        peer_hash_suffix, plain, read_long_pkg_info, render_archy,
    },
};

pub(crate) struct RenderDependentsOptions {
    pub long: bool,
    pub depth: Option<usize>,
}

pub(crate) fn render_dependents_tree(
    trees: &[DependentsTree],
    opts: &RenderDependentsOptions,
) -> String {
    if trees.is_empty() {
        return String::new();
    }

    let multi_peer_pkgs = find_multi_peer_packages(trees);

    let output = trees
        .iter()
        .map(|tree| {
            let displayed_name = tree.display_name.as_deref().unwrap_or(&tree.name);
            let mut root_label_parts = vec![format!(
                "{}{}",
                bold(&name_at_version_plain(displayed_name, &tree.version)),
                peer_hash_suffix(
                    &multi_peer_pkgs,
                    &tree.name,
                    &tree.version,
                    tree.peers_suffix_hash.as_deref(),
                ),
            )];
            if let Some(message) = &tree.search_message {
                root_label_parts.push(plain(message));
            }
            if opts.long
                && let Some(path) = &tree.path
            {
                let info = read_long_pkg_info(Path::new(path));
                if let Some(description) = info.description {
                    root_label_parts.push(plain(&description));
                }
                if let Some(repository) = info.repository {
                    root_label_parts.push(plain(&repository));
                }
                if let Some(homepage) = info.homepage {
                    root_label_parts.push(plain(&homepage));
                }
                root_label_parts.push(plain(path));
            }
            let root_label = root_label_parts.join("\n");
            if tree.dependents.is_empty() {
                return root_label;
            }
            let child_nodes =
                dependents_to_tree_nodes(&tree.dependents, &multi_peer_pkgs, 0, opts.depth);
            let archy = render_archy(&TreeNode::with_children(root_label, child_nodes));
            archy.trim_end_matches('\n').to_string()
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let summary = why_summary(trees);
    if summary.is_empty() { output } else { format!("{output}\n\n{summary}") }
}

fn name_at_version_plain(name: &str, version: &str) -> String {
    name_at_version(name, version, plain)
}

fn why_summary(trees: &[DependentsTree]) -> String {
    if trees.is_empty() {
        return String::new();
    }

    struct Entry {
        versions: Vec<String>,
        count: usize,
    }
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Entry> = HashMap::new();
    for tree in trees {
        let displayed_name = tree.display_name.clone().unwrap_or_else(|| tree.name.clone());
        let entry = by_name.entry(displayed_name.clone()).or_insert_with(|| {
            order.push(displayed_name);
            Entry { versions: Vec::new(), count: 0 }
        });
        if !entry.versions.contains(&tree.version) {
            entry.versions.push(tree.version.clone());
        }
        entry.count += 1;
    }

    let lines: Vec<String> = order
        .iter()
        .map(|name| {
            let entry = &by_name[name];
            let versions = entry.versions.len();
            let mut parts =
                vec![format!("{versions} version{}", if versions == 1 { "" } else { "s" })];
            if entry.count > versions {
                parts.push(format!("{} instances", entry.count));
            }
            format!("Found {} of {name}", parts.join(", "))
        })
        .collect();
    dim(&lines.join("\n"))
}

fn find_multi_peer_packages(trees: &[DependentsTree]) -> HashMap<String, usize> {
    let mut variants = PeerVariants::default();
    fn walk(variants: &mut PeerVariants, dependents: &[DependentNode]) {
        for dep in dependents {
            variants.collect(&dep.name, &dep.version, dep.peers_suffix_hash.as_deref());
            if let Some(children) = &dep.dependents {
                walk(variants, children);
            }
        }
    }
    for tree in trees {
        variants.collect(&tree.name, &tree.version, tree.peers_suffix_hash.as_deref());
        walk(&mut variants, &tree.dependents);
    }
    variants.into_multi_variant_counts()
}

fn dependents_to_tree_nodes(
    dependents: &[DependentNode],
    multi_peer_pkgs: &HashMap<String, usize>,
    current_depth: usize,
    max_depth: Option<usize>,
) -> Vec<TreeNode> {
    dependents
        .iter()
        .map(|dep| {
            let displayed_name = dep.display_name.as_deref().unwrap_or(&dep.name);
            let mut label = if let Some(dep_field) = dep.dep_field {
                // An importer (leaf node).
                format!(
                    "{} {}",
                    bold(&name_at_version_plain(displayed_name, &dep.version)),
                    dim(&format!("({})", dep_field.as_str())),
                )
            } else {
                format!(
                    "{}{}",
                    name_at_version_plain(displayed_name, &dep.version),
                    peer_hash_suffix(
                        multi_peer_pkgs,
                        &dep.name,
                        &dep.version,
                        dep.peers_suffix_hash.as_deref(),
                    ),
                )
            };
            if dep.circular {
                label.push_str(&circular_label());
            }
            if dep.deduped {
                label.push_str(&deduped_label());
            }

            let at_depth_limit = max_depth.is_some_and(|max_depth| current_depth + 1 >= max_depth);
            let nodes = match &dep.dependents {
                Some(children) if !at_depth_limit => dependents_to_tree_nodes(
                    children,
                    multi_peer_pkgs,
                    current_depth + 1,
                    max_depth,
                ),
                _ => Vec::new(),
            };
            TreeNode::with_children(label, nodes)
        })
        .collect()
}

pub(crate) fn render_dependents_json(
    trees: &[DependentsTree],
    opts: &RenderDependentsOptions,
) -> String {
    let values: Vec<serde_json::Value> = trees
        .iter()
        .map(|tree| {
            let mut tree = tree.clone();
            if let Some(max_depth) = opts.depth {
                tree.dependents = truncate_dependents(tree.dependents, 0, max_depth);
            }
            let mut value = serde_json::to_value(&tree).expect("serialize dependents tree");
            if opts.long
                && let Some(path) = &tree.path
                && let Some(object) = value.as_object_mut()
            {
                let info = read_long_pkg_info(Path::new(path));
                if let Some(description) = info.description {
                    object.insert("description".to_string(), serde_json::json!(description));
                }
                if let Some(repository) = info.repository {
                    object.insert("repository".to_string(), serde_json::json!(repository));
                }
                if let Some(homepage) = info.homepage {
                    object.insert("homepage".to_string(), serde_json::json!(homepage));
                }
            }
            value
        })
        .collect();
    serde_json::to_string_pretty(&values).expect("serialize dependents trees")
}

fn truncate_dependents(
    dependents: Vec<DependentNode>,
    current_depth: usize,
    max_depth: usize,
) -> Vec<DependentNode> {
    dependents
        .into_iter()
        .map(|mut dep| {
            dep.dependents = match dep.dependents {
                Some(children) if current_depth + 1 < max_depth => {
                    Some(truncate_dependents(children, current_depth + 1, max_depth))
                }
                _ => None,
            };
            dep
        })
        .collect()
}

pub(crate) fn render_dependents_parseable(
    trees: &[DependentsTree],
    opts: &RenderDependentsOptions,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for tree in trees {
        let displayed_name = tree.display_name.as_deref().unwrap_or(&tree.name);
        let root_segment = match (&tree.path, opts.long) {
            (Some(path), true) => {
                format!("{path}:{}", plain_name_at_version(displayed_name, &tree.version))
            }
            _ => plain_name_at_version(displayed_name, &tree.version),
        };
        collect_paths(&tree.dependents, &[root_segment], &mut lines, 0, opts.depth);
    }
    lines.join("\n")
}

fn collect_paths(
    dependents: &[DependentNode],
    current_path: &[String],
    lines: &mut Vec<String>,
    current_depth: usize,
    max_depth: Option<usize>,
) {
    for dep in dependents {
        let displayed_name = dep.display_name.as_deref().unwrap_or(&dep.name);
        let mut new_path = current_path.to_vec();
        new_path.push(plain_name_at_version(displayed_name, &dep.version));
        let at_depth_limit = max_depth.is_some_and(|max_depth| current_depth + 1 >= max_depth);
        match &dep.dependents {
            Some(children) if !children.is_empty() && !at_depth_limit => {
                collect_paths(children, &new_path, lines, current_depth + 1, max_depth);
            }
            _ => {
                // Leaf (importer or depth-limited) — reversed so the
                // importer comes first.
                new_path.reverse();
                lines.push(new_path.join(" > "));
            }
        }
    }
}

fn plain_name_at_version(name: &str, version: &str) -> String {
    if version.is_empty() { plain(name) } else { plain(&format!("{name}@{version}")) }
}
