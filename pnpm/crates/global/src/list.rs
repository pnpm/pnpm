//! List the globally installed packages, with tree / JSON / parseable
//! renderers for depth-0 output.
//!
//! Only the direct-dependency (depth 0) shape is needed: global installs
//! list their resolved direct deps under a single private root.

use crate::scan::{get_global_package_details, scan_global_packages};
use owo_colors::{OwoColorize, Stream};
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
    let mut install_dirs: Vec<PathBuf> = Vec::new();
    for pkg in packages {
        let matched = pkg.dependencies.iter().any(|(alias, _)| matches_params(params, alias));
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

    let mut deps: Vec<ListedDep> = Vec::new();
    for pkg in &packages {
        for installed in get_global_package_details(pkg) {
            if !matches_params(params, &installed.alias) {
                continue;
            }
            let name = installed
                .manifest
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&installed.alias)
                .to_string();
            let location = pkg.install_dir.join("node_modules").join(&installed.alias);
            let path = location.to_string_lossy().into_owned();
            deps.push(ListedDep {
                alias: installed.alias.clone(),
                name,
                version: installed.version.clone(),
                location,
                path,
            });
        }
    }
    deps.sort_by(|a, b| a.alias.cmp(&b.alias));

    if deps.is_empty() {
        return Ok(match report_as {
            ListReportAs::Json => {
                let empty =
                    json!([{ "path": global_dir_str, "private": true, "dependencies": {} }]);
                serde_json::to_string_pretty(&empty).expect("serialize empty global list")
            }
            ListReportAs::Parseable => global_dir_str,
            ListReportAs::Tree => {
                if params.is_empty() {
                    "No global packages found".to_string()
                } else {
                    "No matching global packages found".to_string()
                }
            }
        });
    }

    Ok(match report_as {
        ListReportAs::Json => render_json(&global_dir_str, &deps, long),
        ListReportAs::Parseable => render_parseable(&global_dir_str, &deps, long),
        ListReportAs::Tree => render_tree(&global_dir_str, &deps, long),
    })
}

fn render_json(global_dir: &str, deps: &[ListedDep], long: bool) -> String {
    let mut dependencies = Map::new();
    for dep in deps {
        let mut item = Map::new();
        item.insert("from".to_string(), json!(dep.name));
        item.insert("version".to_string(), json!(dep.version));
        if long {
            // `getPkgInfo` reads the dependency's manifest for the extra
            // fields; omit any that are absent (JSON.stringify drops
            // undefined).
            if let Some(manifest) = read_dep_manifest(dep) {
                for (key, source) in [
                    ("description", "description"),
                    ("license", "license"),
                    ("homepage", "homepage"),
                ] {
                    if let Some(value) = manifest.get(source).and_then(Value::as_str) {
                        item.insert(key.to_string(), json!(value));
                    }
                }
                if let Some(repo) = repository_url(&manifest) {
                    item.insert("repository".to_string(), json!(repo));
                }
            }
        }
        item.insert("path".to_string(), json!(dep.path));
        dependencies.insert(dep.alias.clone(), Value::Object(item));
    }
    let root = json!([{
        "path": global_dir,
        "private": true,
        "dependencies": Value::Object(dependencies),
    }]);
    serde_json::to_string_pretty(&root).expect("serialize global list")
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

//  archy tree renderer -------------------------------------------------

struct TreeNode {
    label: String,
    groups: Vec<Group>,
}

struct Group {
    group: String,
    nodes: Vec<TreeNode>,
}

fn render_node(node: &TreeNode, connector: &str, prefix: &str, out: &mut String) {
    let lines: Vec<&str> = node.label.split('\n').collect();
    if !connector.is_empty() {
        out.push_str(&dim(connector));
    }
    out.push_str(lines[0]);
    out.push('\n');

    // Flatten group children into (node, group header) items.
    let mut items: Vec<(&TreeNode, &str)> = Vec::new();
    for group in &node.groups {
        for gn in &group.nodes {
            items.push((gn, group.group.as_str()));
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
    for (i, (item, group)) in items.into_iter().enumerate() {
        let last = i == count - 1;
        if Some(group) != current_group {
            current_group = Some(group);
            out.push_str(&dim(&format!("{prefix}\u{2502}")));
            out.push('\n');
            out.push_str(&dim(&format!("{prefix}\u{2502}   ")));
            out.push_str(group);
            out.push('\n');
        }
        let more = !item.groups.is_empty();
        let branch = if last { "\u{2514}" } else { "\u{251c}" };
        let stem = if more { "\u{252c}" } else { "\u{2500}" };
        let child_connector = format!("{prefix}{branch}\u{2500}{stem} ");
        let child_prefix = if last { format!("{prefix}  ") } else { format!("{prefix}\u{2502} ") };
        render_node(item, &child_connector, &child_prefix, out);
    }
}

//  helpers ------------------------------------------------------------

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

fn matches_params(params: &[String], alias: &str) -> bool {
    if params.is_empty() {
        return true;
    }
    params.iter().any(|pattern| glob_match(pattern, alias))
}

/// Minimal `*`-glob matcher (no negation) covering the package-name
/// patterns `pnpm list` accepts as positional args.
fn glob_match(pattern: &str, value: &str) -> bool {
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
