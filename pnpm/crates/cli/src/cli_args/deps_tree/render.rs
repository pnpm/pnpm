//! Rendering helpers shared by the `list` and `why` output formats:
//! the archy-style tree renderer, color helpers, label formatting, and
//! peer-variant bookkeeping.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use owo_colors::{OwoColorize, Stream};

use crate::cli_args::sanitize::sanitize;

pub(crate) struct TreeNode {
    pub label: String,
    pub groups: Vec<TreeNodeGroup>,
}

pub(crate) struct TreeNodeGroup {
    pub group: String,
    pub nodes: Vec<TreeNode>,
}

impl TreeNode {
    /// A node whose children carry no group header.
    pub(crate) fn with_children(label: String, nodes: Vec<TreeNode>) -> TreeNode {
        let groups = if nodes.is_empty() {
            Vec::new()
        } else {
            vec![TreeNodeGroup { group: String::new(), nodes }]
        };
        TreeNode { label, groups }
    }
}

/// Archy-style tree renderer with dimmed tree-drawing characters,
/// matching the TypeScript `@pnpm/text.tree-renderer` output.
pub(crate) fn render_archy(node: &TreeNode) -> String {
    let mut out = String::new();
    render_archy_node(node, "", "", &mut out);
    out
}

fn render_archy_node(node: &TreeNode, connector: &str, prefix: &str, out: &mut String) {
    let lines: Vec<&str> = node.label.split('\n').collect();
    if !connector.is_empty() {
        out.push_str(&dim(connector));
    }
    out.push_str(lines[0]);
    out.push('\n');

    struct Item<'a> {
        node: &'a TreeNode,
        group: &'a str,
    }

    let mut items: Vec<Item<'_>> = Vec::new();
    for group in &node.groups {
        for group_node in &group.nodes {
            items.push(Item { node: group_node, group: group.group.as_str() });
        }
    }

    let continuation = if items.is_empty() { "  " } else { "\u{2502} " };
    for line in &lines[1..] {
        out.push_str(&dim(&format!("{prefix}{continuation}")));
        out.push_str(line);
        out.push('\n');
    }

    let mut current_group: Option<&str> = None;
    let count = items.len();
    for (i, item) in items.into_iter().enumerate() {
        let last = i == count - 1;

        if current_group != Some(item.group) {
            current_group = Some(item.group);
            if !item.group.is_empty() {
                out.push_str(&dim(&format!("{prefix}\u{2502}")));
                out.push('\n');
                out.push_str(&dim(&format!("{prefix}\u{2502}   ")));
                out.push_str(item.group);
                out.push('\n');
            }
        }

        let more = item.node.groups.iter().any(|group| !group.nodes.is_empty());
        let branch = if last { "\u{2514}" } else { "\u{251c}" };
        let stem = if more { "\u{252c}" } else { "\u{2500}" };
        let child_connector = format!("{prefix}{branch}\u{2500}{stem} ");
        let child_prefix = if last { format!("{prefix}  ") } else { format!("{prefix}\u{2502} ") };
        render_archy_node(item.node, &child_connector, &child_prefix, out);
    }
}

/// A terminal-styling function (identity when colors are off).
pub(crate) type ColorFn = fn(&str) -> String;

/// `name` followed by the gray `@version` suffix (no suffix when the
/// version is empty). `color` styles the name only.
pub(crate) fn name_at_version(name: &str, version: &str, color: ColorFn) -> String {
    let name = sanitize(name);
    let version = sanitize(version);
    if version.is_empty() {
        color(&name)
    } else {
        format!("{}{}", color(&name), gray(&format!("@{version}")))
    }
}

pub(crate) fn deduped_label() -> String {
    dim(" [deduped]")
}

pub(crate) fn circular_label() -> String {
    dim(" [circular]")
}

/// Track how many distinct peer-variant hashes each `name@version`
/// appears with; only packages with more than one variant get the
/// `peer#<hash>` suffix in the output.
#[derive(Debug, Default)]
pub(crate) struct PeerVariants {
    hashes_per_pkg: HashMap<String, HashSet<String>>,
}

impl PeerVariants {
    pub(crate) fn collect(&mut self, name: &str, version: &str, hash: Option<&str>) {
        let Some(hash) = hash else { return };
        self.hashes_per_pkg
            .entry(format!("{name}@{version}"))
            .or_default()
            .insert(hash.to_string());
    }

    pub(crate) fn into_multi_variant_counts(self) -> HashMap<String, usize> {
        self.hashes_per_pkg
            .into_iter()
            .filter(|(_, hashes)| hashes.len() > 1)
            .map(|(key, hashes)| (key, hashes.len()))
            .collect()
    }
}

pub(crate) fn peer_hash_suffix(
    multi_peer_pkgs: &HashMap<String, usize>,
    name: &str,
    version: &str,
    hash: Option<&str>,
) -> String {
    let Some(hash) = hash else { return String::new() };
    let Some(count) = multi_peer_pkgs.get(&format!("{name}@{version}")) else {
        return String::new();
    };
    let plural = if *count == 1 { "" } else { "s" };
    red(&format!(" peer#{hash} ({count} variation{plural})"))
}

/// Extra manifest details shown by `--long`.
#[derive(Debug, Default, Clone)]
pub(crate) struct LongPkgInfo {
    pub description: Option<String>,
    pub license: Option<serde_json::Value>,
    pub author: Option<serde_json::Value>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
}

/// Read the `--long` manifest details from `<pkg_dir>/package.json`.
/// Unreadable manifests degrade to a placeholder description, matching
/// the TypeScript CLI.
pub(crate) fn read_long_pkg_info(pkg_dir: &Path) -> LongPkgInfo {
    let manifest: Option<serde_json::Value> = std::fs::read(pkg_dir.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let Some(manifest) = manifest else {
        return LongPkgInfo {
            description: Some("[Could not find additional info about this dependency]".to_string()),
            ..LongPkgInfo::default()
        };
    };
    LongPkgInfo {
        description: manifest
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        license: manifest.get("license").cloned(),
        author: manifest.get("author").cloned(),
        homepage: manifest.get("homepage").and_then(serde_json::Value::as_str).map(str::to_string),
        repository: match manifest.get("repository") {
            Some(serde_json::Value::String(url)) => Some(url.clone()),
            Some(serde_json::Value::Object(map)) => {
                map.get("url").and_then(serde_json::Value::as_str).map(str::to_string)
            }
            _ => None,
        },
    }
}

pub(crate) fn plain(text: &str) -> String {
    sanitize(text).into_owned()
}

pub(crate) fn dim(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

pub(crate) fn bold(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

pub(crate) fn cyan_bright(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}

pub(crate) fn gray(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bright_black()).to_string()
}

pub(crate) fn yellow(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.yellow()).to_string()
}

pub(crate) fn blue(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.blue()).to_string()
}

pub(crate) fn red(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}
