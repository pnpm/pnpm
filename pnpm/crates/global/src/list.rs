//! List the globally installed packages, with tree / JSON / parseable
//! renderers for depth-0 output.
//!
//! Only the direct-dependency (depth 0) shape is needed: global installs
//! list their resolved direct deps under a single private root.

use crate::scan::{
    GlobalPackageInfo, InstalledGlobalPackage, get_global_package_details, scan_global_packages,
};
use owo_colors::{OwoColorize, Stream};
use pnpm_matcher::WildcardMatcher;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Output format for [`list_global_packages`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListReportAs {
    Tree,
    Json,
    Parseable,
}

/// One resolved global dependency to render.
struct ListedDep {
    alias: String,
    name: String,
    version: String,
    /// Filesystem location of the installed dependency, used for manifest
    /// I/O (kept as a `PathBuf` so non-UTF-8 paths round-trip losslessly).
    location: PathBuf,
    /// Display form of [`Self::location`] for the rendered output.
    path: String,
}

/// The install directories with a direct-dependency alias matching
/// `params` (any alias when `params` is empty — a group with no
/// dependencies never matches, mirroring the TypeScript
/// `findGlobalInstallDirs`). Used by `pnpm ls -g --depth <n>` to narrow
/// the listing to one install group.
pub fn find_global_install_dirs(
    global_dir: &Path,
    params: &[String],
) -> std::io::Result<Vec<PathBuf>> {
    let packages = scan_global_packages(global_dir)?;
    let patterns: Vec<_> = params.iter().map(|pattern| WildcardMatcher::new(pattern)).collect();
    let mut install_dirs: Vec<PathBuf> = Vec::new();
    for pkg in packages {
        let matched = pkg.dependencies.iter().any(|(alias, _)| matches_params(&patterns, alias));
        if matched && !install_dirs.contains(&pkg.install_dir) {
            install_dirs.push(pkg.install_dir);
        }
    }
    Ok(install_dirs)
}

/// Render the globally installed packages matching `params` (all when
/// empty) in the requested format.
pub fn list_global_packages(
    global_dir: &Path,
    params: &[String],
    report_as: ListReportAs,
    long: bool,
) -> std::io::Result<String> {
    let packages = scan_global_packages(global_dir)?;
    let global_dir_str = global_dir.to_string_lossy().into_owned();
    let deps = collect_listed_deps(&packages, params);

    if deps.is_empty() {
        return Ok(render_empty(&global_dir_str, params, report_as));
    }

    Ok(match report_as {
        ListReportAs::Json => render_json(&global_dir_str, &deps, long),
        ListReportAs::Parseable => render_parseable(&global_dir_str, &deps, long),
        ListReportAs::Tree => render_tree(&global_dir_str, &deps, long),
    })
}

/// Every installed dependency matching `params`, sorted by alias.
fn collect_listed_deps(packages: &[GlobalPackageInfo], params: &[String]) -> Vec<ListedDep> {
    let patterns: Vec<_> = params.iter().map(|pattern| WildcardMatcher::new(pattern)).collect();
    let installed = packages.iter().flat_map(|pkg| {
        get_global_package_details(pkg).into_iter().map(move |installed| (pkg, installed))
    });
    let mut deps: Vec<ListedDep> = installed
        .filter(|(_, installed)| matches_params(&patterns, &installed.alias))
        .map(|(pkg, installed)| listed_dep(pkg, installed))
        .collect();
    deps.sort_by(|a, b| a.alias.cmp(&b.alias));
    deps
}

fn listed_dep(pkg: &GlobalPackageInfo, installed: InstalledGlobalPackage) -> ListedDep {
    let name = installed
        .manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&installed.alias)
        .to_string();
    let location = pkg.install_dir.join("node_modules").join(&installed.alias);
    let path = location.to_string_lossy().into_owned();
    ListedDep { alias: installed.alias, name, version: installed.version, location, path }
}

fn render_empty(global_dir: &str, params: &[String], report_as: ListReportAs) -> String {
    match report_as {
        ListReportAs::Json => {
            let empty = json!([{ "path": global_dir, "private": true, "dependencies": {} }]);
            serde_json::to_string_pretty(&empty).expect("serialize empty global list")
        }
        ListReportAs::Parseable => global_dir.to_string(),
        ListReportAs::Tree if params.is_empty() => "No global packages found".to_string(),
        ListReportAs::Tree => "No matching global packages found".to_string(),
    }
}

fn render_json(global_dir: &str, deps: &[ListedDep], long: bool) -> String {
    let mut dependencies = Map::new();
    for dep in deps {
        dependencies.insert(dep.alias.clone(), Value::Object(json_item(dep, long)));
    }
    let root = json!([{
        "path": global_dir,
        "private": true,
        "dependencies": Value::Object(dependencies),
    }]);
    serde_json::to_string_pretty(&root).expect("serialize global list")
}

fn json_item(dep: &ListedDep, long: bool) -> Map<String, Value> {
    let mut item = Map::new();
    item.insert("from".to_string(), json!(dep.name));
    item.insert("version".to_string(), json!(dep.version));
    if long {
        insert_manifest_fields(&mut item, dep);
    }
    item.insert("path".to_string(), json!(dep.path));
    item
}

/// `getPkgInfo` reads the dependency's manifest for the extra fields; any
/// that are absent stay out of the object, as `JSON.stringify` drops
/// `undefined`.
fn insert_manifest_fields(item: &mut Map<String, Value>, dep: &ListedDep) {
    let Some(manifest) = read_dep_manifest(dep) else {
        return;
    };
    for key in ["description", "license", "homepage"] {
        if let Some(value) = manifest.get(key).and_then(Value::as_str) {
            item.insert(key.to_string(), json!(value));
        }
    }
    if let Some(repo) = repository_url(&manifest) {
        item.insert("repository".to_string(), json!(repo));
    }
}

fn render_parseable(global_dir: &str, deps: &[ListedDep], long: bool) -> String {
    let mut lines = vec![global_dir.to_string()];
    for dep in deps {
        if long {
            lines.push(parseable_long_line(dep));
        } else {
            lines.push(dep.path.clone());
        }
    }
    lines.join("\n")
}

/// `--parseable --long` line for one dependency, using the alias-aware
/// `path:locator` form.
fn parseable_long_line(dep: &ListedDep) -> String {
    if dep.alias != dep.name {
        // npm-aliased dependency: emit the alias, plus an `npm:` locator
        // unless the version is already a full `name@spec` form.
        if dep.version.contains('@') {
            return format!("{}:{} {}", dep.path, dep.alias, dep.version);
        }
        return format!("{}:{} npm:{}@{}", dep.path, dep.alias, dep.name, dep.version);
    }
    if dep.version.contains('@') {
        return format!("{}:{}", dep.path, dep.version);
    }
    format!("{}:{}@{}", dep.path, dep.name, dep.version)
}

const LEGEND: &str = "Legend: production dependency, optional only, dev only\n\n";

fn render_tree(global_dir: &str, deps: &[ListedDep], long: bool) -> String {
    let root_label = bold(&format!("{}{}", dim(global_dir), dim(" (PRIVATE)")));

    let mut leaves = Vec::with_capacity(deps.len());
    for dep in deps {
        let mut label = leaf_label(dep);
        if long && let Some(manifest) = read_dep_manifest(dep) {
            for value in [
                manifest.get("description").and_then(Value::as_str).map(str::to_string),
                repository_url(&manifest),
                manifest.get("homepage").and_then(Value::as_str).map(str::to_string),
                Some(dep.path.clone()),
            ]
            .into_iter()
            .flatten()
            {
                label.push('\n');
                label.push_str(&value);
            }
        }
        leaves.push(TreeNode { label, groups: Vec::new() });
    }

    let root = TreeNode {
        label: root_label,
        groups: vec![Group { group: cyan_bright("dependencies:"), nodes: leaves }],
    };
    let mut out = String::new();
    render_node(&root, "", "", &mut out);
    format!("{LEGEND}{}", out.trim_end())
}

/// Leaf label for a non-peer, non-deduped node in the `dependencies`
/// group (always production color, i.e. uncolored).
fn leaf_label(dep: &ListedDep) -> String {
    if dep.alias != dep.name {
        // npm-aliased dependency.
        if !dep.version.contains('@') {
            return format!("{}{}", dep.alias, gray(&format!("@npm:{}@{}", dep.name, dep.version)));
        }
        return format!("{}{}", dep.alias, gray(&format!("@{}", dep.version)));
    }
    if dep.version.is_empty() {
        return dep.name.clone();
    }
    format!("{}{}", dep.name, gray(&format!("@{}", dep.version)))
}

// --- archy tree renderer ----------------------------------------------------

struct TreeNode {
    label: String,
    groups: Vec<Group>,
}

struct Group {
    group: String,
    nodes: Vec<TreeNode>,
}

fn render_node(node: &TreeNode, connector: &str, prefix: &str, out: &mut String) {
    let items = flatten_groups(node);
    push_label(&node.label, connector, prefix, items.is_empty(), out);
    render_children(&items, prefix, out);
}

/// The group children in display order, each paired with the header its group
/// prints above it.
fn flatten_groups(node: &TreeNode) -> Vec<(&TreeNode, &str)> {
    node.groups
        .iter()
        .flat_map(|group| group.nodes.iter().map(|node| (node, group.group.as_str())))
        .collect()
}

fn push_label(label: &str, connector: &str, prefix: &str, leaf: bool, out: &mut String) {
    let lines: Vec<&str> = label.split('\n').collect();
    if !connector.is_empty() {
        out.push_str(&dim(connector));
    }
    out.push_str(lines[0]);
    out.push('\n');

    let continuation = if leaf { "  " } else { "\u{2502} " };
    for line in &lines[1..] {
        out.push_str(&dim(&format!("{prefix}{continuation}")));
        out.push_str(line);
        out.push('\n');
    }
}

fn render_children(items: &[(&TreeNode, &str)], prefix: &str, out: &mut String) {
    let mut current_group: Option<&str> = None;
    for (index, (item, group)) in items.iter().enumerate() {
        if current_group != Some(group) {
            current_group = Some(group);
            push_group_header(group, prefix, out);
        }
        let (connector, child_prefix) =
            child_frames(prefix, index + 1 == items.len(), !item.groups.is_empty());
        render_node(item, &connector, &child_prefix, out);
    }
}

fn push_group_header(group: &str, prefix: &str, out: &mut String) {
    out.push_str(&dim(&format!("{prefix}\u{2502}")));
    out.push('\n');
    out.push_str(&dim(&format!("{prefix}\u{2502}   ")));
    out.push_str(group);
    out.push('\n');
}

/// The connector drawn before a child and the prefix its own children inherit.
/// `last` picks the corner glyph, `parent` the downward stem.
fn child_frames(prefix: &str, last: bool, parent: bool) -> (String, String) {
    let branch = if last { "\u{2514}" } else { "\u{251c}" };
    let stem = if parent { "\u{252c}" } else { "\u{2500}" };
    let child_prefix = if last { format!("{prefix}  ") } else { format!("{prefix}\u{2502} ") };
    (format!("{prefix}{branch}\u{2500}{stem} "), child_prefix)
}

// --- helpers ---------------------------------------------------------------

fn read_dep_manifest(dep: &ListedDep) -> Option<Value> {
    crate::read_package_json(&dep.location)
}

fn repository_url(manifest: &Value) -> Option<String> {
    match manifest.get("repository") {
        Some(Value::String(url)) => Some(url.clone()),
        Some(Value::Object(map)) => map.get("url").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn matches_params(patterns: &[WildcardMatcher], alias: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| pattern.matches(alias))
}

fn dim(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn cyan_bright(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}

fn gray(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_black()).to_string()
}

#[cfg(test)]
mod tests;
