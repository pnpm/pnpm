//! `pnpm why` output renderers (tree / parseable / JSON), mirroring the
//! TypeScript `renderDependentsTree` / `renderDependentsJson` /
//! `renderDependentsParseable`.

use std::{collections::HashMap, path::Path};

use crate::{
    dependents::{DependentNode, DependentsTree},
    render::{
        LongPkgInfo, PeerVariants, TreeNode, bold_styled, circular_label, deduped_label, dim,
        name_at_version, peer_hash_suffix, plain, read_long_pkg_info, render_archy,
    },
};

/// Shared by the three renderers below; each ignores what does not apply
/// to its format (`long` reads each root's `package.json`, which the
/// parseable and JSON formats do not render).
pub struct RenderDependentsOptions {
    /// Include the description, repository, homepage, and path of each
    /// matched package under its root label.
    pub long: bool,
    /// Stop descending after this many levels of dependents. `None`
    /// renders the whole tree.
    pub depth: Option<usize>,
}

#[must_use]
pub fn render_dependents_tree(trees: &[DependentsTree], opts: &RenderDependentsOptions) -> String {
    if trees.is_empty() {
        return String::new();
    }

    let multi_peer_pkgs = find_multi_peer_packages(trees);
    let output = trees
        .iter()
        .map(|tree| render_one_tree(tree, &multi_peer_pkgs, opts))
        .collect::<Vec<_>>()
        .join("\n\n");

    let summary = why_summary(trees);
    if summary.is_empty() { output } else { format!("{output}\n\n{summary}") }
}

fn render_one_tree(
    tree: &DependentsTree,
    multi_peer_pkgs: &HashMap<String, usize>,
    opts: &RenderDependentsOptions,
) -> String {
    let root_label = root_label(tree, multi_peer_pkgs, opts.long);
    if tree.dependents.is_empty() {
        return root_label;
    }
    let child_nodes = dependents_to_tree_nodes(&tree.dependents, multi_peer_pkgs, 0, opts.depth);
    let archy = render_archy(&TreeNode::with_children(root_label, child_nodes));
    archy.trim_end_matches('\n').to_string()
}

fn root_label(
    tree: &DependentsTree,
    multi_peer_pkgs: &HashMap<String, usize>,
    long: bool,
) -> String {
    let displayed_name = tree.display_name.as_deref().unwrap_or(&tree.name);
    let mut parts = vec![format!(
        "{}{}",
        bold_styled(&name_at_version_plain(displayed_name, &tree.version)),
        peer_hash_suffix(
            multi_peer_pkgs,
            &tree.name,
            &tree.version,
            tree.peers_suffix_hash.as_deref(),
        ),
    )];
    if let Some(message) = &tree.search_message {
        parts.push(plain(message));
    }
    if long && let Some(path) = &tree.path {
        let info = read_long_pkg_info(Path::new(path));
        parts.extend(long_info_fields(&info).into_iter().map(|field| plain(&field)));
        parts.push(plain(path));
    }
    parts.join("\n")
}

/// The `--long` manifest fields a package has, in display order.
fn long_info_fields(info: &LongPkgInfo) -> Vec<String> {
    [&info.description, &info.repository, &info.homepage].into_iter().flatten().cloned().collect()
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
    let at_depth_limit = max_depth.is_some_and(|max_depth| current_depth + 1 >= max_depth);
    dependents
        .iter()
        .map(|dep| {
            let nodes = match &dep.dependents {
                Some(children) if !at_depth_limit => dependents_to_tree_nodes(
                    children,
                    multi_peer_pkgs,
                    current_depth + 1,
                    max_depth,
                ),
                _ => Vec::new(),
            };
            TreeNode::with_children(dependent_label(dep, multi_peer_pkgs), nodes)
        })
        .collect()
}

fn dependent_label(dep: &DependentNode, multi_peer_pkgs: &HashMap<String, usize>) -> String {
    let displayed_name = dep.display_name.as_deref().unwrap_or(&dep.name);
    let mut label = match dep.dep_field {
        // An importer (leaf node).
        Some(dep_field) => format!(
            "{} {}",
            bold_styled(&name_at_version_plain(displayed_name, &dep.version)),
            dim(&format!("({})", dep_field.as_str())),
        ),
        None => format!(
            "{}{}",
            name_at_version_plain(displayed_name, &dep.version),
            peer_hash_suffix(
                multi_peer_pkgs,
                &dep.name,
                &dep.version,
                dep.peers_suffix_hash.as_deref(),
            ),
        ),
    };
    if dep.circular {
        label.push_str(&circular_label());
    }
    if dep.deduped {
        label.push_str(&deduped_label());
    }
    label
}

#[must_use]
pub fn render_dependents_json(trees: &[DependentsTree], opts: &RenderDependentsOptions) -> String {
    let values: Vec<serde_json::Value> = trees
        .iter()
        .map(|tree| {
            let mut tree = tree.clone();
            if let Some(max_depth) = opts.depth {
                tree.dependents = truncate_dependents(tree.dependents, 0, max_depth);
            }
            let mut value = serde_json::to_value(&tree).expect("serialize dependents tree");
            if opts.long {
                insert_long_info(&mut value, tree.path.as_deref());
            }
            value
        })
        .collect();
    serde_json::to_string_pretty(&values).expect("serialize dependents trees")
}

/// Add the `--long` manifest fields to a serialized tree, under the keys
/// `pnpm why --json` uses.
fn insert_long_info(value: &mut serde_json::Value, path: Option<&str>) {
    let (Some(path), Some(object)) = (path, value.as_object_mut()) else {
        return;
    };
    let info = read_long_pkg_info(Path::new(path));
    for (key, field) in [
        ("description", info.description),
        ("repository", info.repository),
        ("homepage", info.homepage),
    ] {
        if let Some(field) = field {
            object.insert(key.to_string(), serde_json::json!(field));
        }
    }
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

#[must_use]
pub fn render_dependents_parseable(
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

#[cfg(test)]
mod tests;
