use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use clap::Args;
use miette::{Context, IntoDiagnostic};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::Config;
use pacquet_global::{ListReportAs, list_global_packages};
use pacquet_lockfile::{Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotEntry};
use serde_json::{Map, Value, json};

#[derive(Debug, Args)]
pub struct ListArgs {
    pub packages: Vec<String>,

    #[clap(short = 'g', long)]
    pub global: bool,

    #[clap(long)]
    pub long: bool,

    #[clap(long)]
    pub json: bool,

    #[clap(long)]
    pub parseable: bool,

    #[clap(long, default_value_t = 0)]
    pub depth: i32,

    #[clap(short = 'P', long = "prod")]
    pub production: bool,

    #[clap(short = 'D', long)]
    pub dev: bool,

    #[clap(long)]
    pub no_optional: bool,

    #[clap(long)]
    pub exclude_peers: bool,

    #[clap(long)]
    pub lockfile_only: bool,
}

impl ListArgs {
    pub fn run(self, config: &Config, dir: &Path) -> miette::Result<()> {
        if self.global {
            return self.run_global(config);
        }
        self.run_local(config, dir)
    }

    fn run_global(&self, config: &Config) -> miette::Result<()> {
        let global_pkg_dir = config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                "Unable to find the global packages directory"
            )
        })?;

        let report_as = if self.json {
            ListReportAs::Json
        } else if self.parseable {
            ListReportAs::Parseable
        } else {
            ListReportAs::Tree
        };

        let output = list_global_packages(&global_pkg_dir, &self.packages, report_as, self.long)
            .into_diagnostic()
            .wrap_err("list global packages")?;
        println!("{output}");
        Ok(())
    }

    fn run_local(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
        let lockfile_result = if self.lockfile_only {
            Lockfile::load_wanted_from_dir(lockfile_dir)
        } else {
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
                Ok(Some(lf)) => Ok(Some(lf)),
                _ => Lockfile::load_wanted_from_dir(lockfile_dir),
            }
        };
        let Some(lockfile) = lockfile_result.into_diagnostic().wrap_err("load lockfile")? else {
            if self.packages.is_empty() {
                println!("No lockfile found in {}", lockfile_dir.display());
            } else {
                println!("No matching packages found");
            }
            return Ok(());
        };

        let importer_id: String = if dir == lockfile_dir {
            ".".into()
        } else {
            dir.strip_prefix(lockfile_dir)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| ".".to_string())
        };
        let Some(importer) =
            lockfile.importers.get(&importer_id).or_else(|| lockfile.root_project())
        else {
            if self.packages.is_empty() {
                println!("No packages found");
            } else {
                println!("No matching packages found");
            }
            return Ok(());
        };

        let manifest_path = dir.join("package.json");
        let (root_name, root_version, root_private) = read_root_manifest(&manifest_path)?;

        let prod_explicitly_false = self.dev && !self.production;
        let include_prod = !prod_explicitly_false;
        let include_dev = !self.production;
        let include_optional = !self.no_optional && !prod_explicitly_false;

        let ctx = BuildContext {
            snapshots: lockfile.snapshots.as_ref(),
            packages: lockfile.packages.as_ref(),
            depth: self.depth,
            exclude_peers: self.exclude_peers,
            include_optional,
            virtual_store_dir_max_length: config.virtual_store_dir_max_length as usize,
        };

        let mut tree =
            build_local_tree(importer, &ctx, include_prod, include_dev, include_optional);

        if !self.packages.is_empty() {
            let matches = |dep: &DepNode| dep_or_subtree_matches(dep, &self.packages);
            tree.dependencies.retain(|dep| matches(dep));
            tree.dev_dependencies.retain(|dep| matches(dep));
            tree.optional_dependencies.retain(|dep| matches(dep));

            if tree.dependencies.is_empty()
                && tree.dev_dependencies.is_empty()
                && tree.optional_dependencies.is_empty()
            {
                println!("No matching packages found");
                return Ok(());
            }
        }

        let root = LocalTreeRoot {
            name: root_name,
            version: root_version,
            private: root_private,
            path: dir.to_string_lossy().into_owned(),
            dependencies: tree.dependencies,
            dev_dependencies: tree.dev_dependencies,
            optional_dependencies: tree.optional_dependencies,
        };

        if self.json {
            let output = render_local_json(&root);
            println!("{output}");
        } else if self.parseable {
            let output = render_local_parseable(&root, self.long);
            println!("{output}");
        } else {
            let output = render_local_tree(&root, self.long);
            if !output.is_empty() {
                println!("{output}");
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct DepNode {
    alias: String,
    name: String,
    version: String,
    path: String,
    is_peer: bool,
    is_dev: bool,
    is_optional: bool,
    dependencies: Vec<DepNode>,
}

#[derive(Debug)]
struct LocalTreeRoot {
    name: Option<String>,
    version: Option<String>,
    private: Option<bool>,
    path: String,
    dependencies: Vec<DepNode>,
    dev_dependencies: Vec<DepNode>,
    optional_dependencies: Vec<DepNode>,
}

struct BuiltTree {
    dependencies: Vec<DepNode>,
    dev_dependencies: Vec<DepNode>,
    optional_dependencies: Vec<DepNode>,
}

struct BuildContext<'a> {
    snapshots: Option<&'a HashMap<PkgNameVerPeer, SnapshotEntry>>,
    packages: Option<&'a HashMap<PkgNameVerPeer, pacquet_lockfile::PackageMetadata>>,
    depth: i32,
    exclude_peers: bool,
    include_optional: bool,
    virtual_store_dir_max_length: usize,
}

fn build_local_tree(
    importer: &ProjectSnapshot,
    ctx: &BuildContext,
    include_prod: bool,
    include_dev: bool,
    include_optional: bool,
) -> BuiltTree {
    let mut deps = Vec::new();
    let mut dev_deps = Vec::new();
    let mut opt_deps = Vec::new();

    if include_prod && let Some(dep_map) = &importer.dependencies {
        for (name, spec) in dep_map {
            if let Some(node) = resolve_importer_dep(name, spec, ctx, 0) {
                deps.push(node);
            }
        }
    }

    if include_dev && let Some(dep_map) = &importer.dev_dependencies {
        for (name, spec) in dep_map {
            let mut node = resolve_importer_dep(name, spec, ctx, 0);
            if let Some(ref mut n) = node {
                n.is_dev = true;
            }
            if let Some(n) = node {
                dev_deps.push(n);
            }
        }
    }

    if include_optional && let Some(dep_map) = &importer.optional_dependencies {
        for (name, spec) in dep_map {
            let mut node = resolve_importer_dep(name, spec, ctx, 0);
            if let Some(ref mut n) = node {
                n.is_optional = true;
            }
            if let Some(n) = node {
                opt_deps.push(n);
            }
        }
    }

    sort_deps(&mut deps);
    sort_deps(&mut dev_deps);
    sort_deps(&mut opt_deps);

    BuiltTree { dependencies: deps, dev_dependencies: dev_deps, optional_dependencies: opt_deps }
}

fn resolve_importer_dep(
    name: &PkgName,
    spec: &pacquet_lockfile::ResolvedDependencySpec,
    ctx: &BuildContext,
    current_depth: i32,
) -> Option<DepNode> {
    let alias = name.to_string();
    let version_str = spec.version.to_string();
    let snapshot_key = spec.version.resolved_key(name);

    let (resolved_name, resolved_version, is_peer, child_deps) = match &snapshot_key {
        Some(key) => {
            let dep_name = key.name.to_string();
            let dep_version = key.suffix.version().to_string();

            let peer = ctx.exclude_peers
                && ctx.packages.is_some_and(|pkgs| {
                    let base_key = key.without_peer();
                    pkgs.get(&base_key)
                        .and_then(|meta| meta.peer_dependencies.as_ref())
                        .is_some_and(|peers| peers.contains_key(&name.to_string()))
                });

            let children = if current_depth < ctx.depth && ctx.depth >= 0 {
                resolve_snapshot_children(key, ctx, current_depth + 1)
            } else {
                Vec::new()
            };

            (dep_name, dep_version, peer, children)
        }
        None => (alias.clone(), version_str, false, Vec::new()),
    };

    let pkg_path = if let Some(key) = &snapshot_key {
        let vsn = key.to_virtual_store_name(ctx.virtual_store_dir_max_length);
        format!(".pnpm/{vsn}/node_modules/{resolved_name}")
    } else {
        String::new()
    };

    Some(DepNode {
        alias,
        name: resolved_name,
        version: resolved_version,
        path: pkg_path,
        is_peer,
        is_dev: false,
        is_optional: false,
        dependencies: child_deps,
    })
}

fn resolve_snapshot_children(
    key: &PkgNameVerPeer,
    ctx: &BuildContext,
    current_depth: i32,
) -> Vec<DepNode> {
    let Some(snapshots) = ctx.snapshots else { return Vec::new() };
    let Some(entry) = snapshots.get(key) else { return Vec::new() };

    let peer_set = get_peer_set(key, ctx.packages);

    let mut children = Vec::new();

    let mut push_child = |dep_alias: &PkgName, dep_ref: &pacquet_lockfile::SnapshotDepRef| {
        let child_key = dep_ref.resolve(dep_alias);
        let alias_str = dep_alias.to_string();

        let is_peer = ctx.exclude_peers && peer_set.contains(&alias_str);
        if is_peer {
            return;
        }

        let (child_name, child_version) = match &child_key {
            Some(ck) => (ck.name.to_string(), ck.suffix.version().to_string()),
            None => (alias_str.clone(), dep_ref.to_string()),
        };

        let grandchildren = if current_depth < ctx.depth && ctx.depth >= 0 {
            if let Some(ck) = &child_key {
                resolve_snapshot_children(ck, ctx, current_depth + 1)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let pkg_path = if let Some(ck) = &child_key {
            let vsn = ck.to_virtual_store_name(ctx.virtual_store_dir_max_length);
            format!(".pnpm/{vsn}/node_modules/{child_name}")
        } else {
            String::new()
        };

        children.push(DepNode {
            alias: alias_str,
            name: child_name,
            version: child_version,
            path: pkg_path,
            is_peer,
            is_dev: false,
            is_optional: false,
            dependencies: grandchildren,
        });
    };

    if let Some(deps) = &entry.dependencies {
        for (dep_alias, dep_ref) in deps {
            push_child(dep_alias, dep_ref);
        }
    }

    if ctx.include_optional
        && let Some(opt_deps) = &entry.optional_dependencies
    {
        for (dep_alias, dep_ref) in opt_deps {
            push_child(dep_alias, dep_ref);
        }
    }

    sort_deps(&mut children);
    children
}

fn get_peer_set(
    key: &PkgNameVerPeer,
    packages: Option<&HashMap<PkgNameVerPeer, pacquet_lockfile::PackageMetadata>>,
) -> HashSet<String> {
    let Some(packages) = packages else { return HashSet::new() };
    let base_key = key.without_peer();
    packages
        .get(&base_key)
        .and_then(|meta| meta.peer_dependencies.as_ref())
        .map(|peers| peers.keys().cloned().collect())
        .unwrap_or_default()
}

fn sort_deps(deps: &mut [DepNode]) {
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    for dep in deps.iter_mut() {
        sort_deps(&mut dep.dependencies);
    }
}

fn matches_params(params: &[String], alias: &str) -> bool {
    if params.is_empty() {
        return true;
    }
    params.iter().any(|pattern| glob_match(pattern, alias))
}

fn dep_or_subtree_matches(dep: &DepNode, params: &[String]) -> bool {
    if params.is_empty() {
        return true;
    }
    if matches_params(params, &dep.alias) || matches_params(params, &dep.name) {
        return true;
    }
    dep.dependencies.iter().any(|child| dep_or_subtree_matches(child, params))
}

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

fn read_root_manifest(
    manifest_path: &Path,
) -> miette::Result<(Option<String>, Option<String>, Option<bool>)> {
    let content = std::fs::read_to_string(manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let parsed: Value = serde_json::from_str(&content)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    let name = parsed.get("name").and_then(Value::as_str).map(str::to_string);
    let version = parsed.get("version").and_then(Value::as_str).map(str::to_string);
    let private = parsed.get("private").and_then(Value::as_bool);
    Ok((name, version, private))
}

const LEGEND: &str = "Legend: production dependency, optional only, dev only\n\n";

struct TreeNode {
    label: String,
    groups: Vec<TreeNodeGroup>,
}

struct TreeNodeGroup {
    group: String,
    nodes: Vec<TreeNode>,
}

fn render_local_tree(root: &LocalTreeRoot, long: bool) -> String {
    let mut root_label = String::new();
    if let Some(ref name) = root.name {
        use std::fmt::Write;
        let _ = write!(
            root_label,
            "{} {}",
            name_at_version(name, root.version.as_deref().unwrap_or("")),
            dim(&root.path),
        );
    } else {
        root_label.push_str(&dim(&root.path));
    }
    if root.private == Some(true) {
        root_label.push_str(&dim(" (PRIVATE)"));
    }

    let mut groups: Vec<TreeNodeGroup> = Vec::new();

    if !root.dependencies.is_empty() {
        let nodes = deps_to_tree_nodes(&root.dependencies, long);
        groups.push(TreeNodeGroup { group: cyan_bright("dependencies:"), nodes });
    }

    if !root.dev_dependencies.is_empty() {
        let nodes = deps_to_tree_nodes_colored(&root.dev_dependencies, long, true, false);
        groups.push(TreeNodeGroup { group: cyan_bright("devDependencies:"), nodes });
    }

    if !root.optional_dependencies.is_empty() {
        let nodes = deps_to_tree_nodes_colored(&root.optional_dependencies, long, false, true);
        groups.push(TreeNodeGroup { group: cyan_bright("optionalDependencies:"), nodes });
    }

    if groups.is_empty() {
        return root_label;
    }

    let root_node = TreeNode { label: bold(&root_label), groups };
    let mut output = String::new();
    render_archy(&root_node, "", "", &mut output);
    let body = output.trim_end().to_string();

    if body.is_empty() {
        return String::new();
    }

    format!("{LEGEND}{body}")
}

fn deps_to_tree_nodes(deps: &[DepNode], long: bool) -> Vec<TreeNode> {
    deps_to_tree_nodes_colored(deps, long, false, false)
}

fn deps_to_tree_nodes_colored(
    deps: &[DepNode],
    long: bool,
    is_dev: bool,
    is_optional: bool,
) -> Vec<TreeNode> {
    deps.iter().map(|dep| dep_to_tree_node(dep, long, is_dev, is_optional)).collect()
}

fn dep_to_tree_node(dep: &DepNode, long: bool, is_dev: bool, is_optional: bool) -> TreeNode {
    let color: fn(&str) -> String = if is_optional {
        |text| text.if_supports_color(Stream::Stdout, |text| text.blue()).to_string()
    } else if is_dev {
        |text| text.if_supports_color(Stream::Stdout, |text| text.yellow()).to_string()
    } else {
        |text| text.to_string()
    };

    let mut label = print_label(dep, &color);

    if long && !dep.path.is_empty() {
        label.push('\n');
        label.push_str(&dim(&dep.path));
    }

    let mut groups = Vec::new();
    if !dep.dependencies.is_empty() && !dep.is_peer {
        let children: Vec<TreeNode> = dep
            .dependencies
            .iter()
            .map(|child| dep_to_tree_node(child, long, child.is_dev, child.is_optional))
            .collect();
        groups.push(TreeNodeGroup { group: String::new(), nodes: children });
    }

    TreeNode { label, groups }
}

fn print_label(dep: &DepNode, color: &dyn Fn(&str) -> String) -> String {
    if dep.alias == dep.name {
        format!("{}{}", color(&dep.name), gray(&format!("@{version}", version = dep.version)))
    } else if dep.version.contains('@') {
        format!("{}{}", color(&dep.alias), gray(&format!("@{}", dep.version)))
    } else {
        format!("{}{}", color(&dep.alias), gray(&format!("@npm:{}@{}", dep.name, dep.version)))
    }
}

fn name_at_version(name: &str, version: &str) -> String {
    if version.is_empty() {
        name.to_string()
    } else {
        format!("{}{}", name, gray(&format!("@{version}")))
    }
}

fn render_archy(node: &TreeNode, connector: &str, prefix: &str, out: &mut String) {
    let lines: Vec<&str> = node.label.split('\n').collect();
    if !connector.is_empty() {
        out.push_str(&dim(connector));
    }
    out.push_str(lines[0]);
    out.push('\n');

    struct Item<'a> {
        node: &'a TreeNode,
        group_header: Option<&'a str>,
    }

    let mut items: Vec<Item> = Vec::new();
    for group in &node.groups {
        for (i, gn) in group.nodes.iter().enumerate() {
            items.push(Item { node: gn, group_header: (i == 0).then_some(group.group.as_str()) });
        }
    }

    let continuation = if items.is_empty() { "  " } else { "\u{2502} " };
    for line in &lines[1..] {
        out.push_str(&dim(&format!("{prefix}{continuation}")));
        out.push_str(line);
        out.push('\n');
    }

    let count = items.len();
    for (i, item) in items.into_iter().enumerate() {
        let last = i == count - 1;

        if let Some(header) = item.group_header
            && !header.is_empty()
        {
            out.push_str(&dim(&format!("{prefix}\u{2502}")));
            out.push('\n');
            out.push_str(&dim(&format!("{prefix}\u{2502}   ")));
            out.push_str(header);
            out.push('\n');
        }

        let more = !item.node.groups.is_empty();
        let branch = if last { "\u{2514}" } else { "\u{251c}" };
        let stem = if more { "\u{252c}" } else { "\u{2500}" };
        let child_connector = format!("{prefix}{branch}\u{2500}{stem} ");
        let child_prefix = if last { format!("{prefix}  ") } else { format!("{prefix}\u{2502} ") };
        render_archy(item.node, &child_connector, &child_prefix, out);
    }
}

fn render_local_json(root: &LocalTreeRoot) -> String {
    let mut deps_map = Map::new();
    for dep in &root.dependencies {
        deps_map.insert(dep.alias.clone(), dep_to_json(dep));
    }
    let mut dev_map = Map::new();
    for dep in &root.dev_dependencies {
        dev_map.insert(dep.alias.clone(), dep_to_json(dep));
    }
    let mut opt_map = Map::new();
    for dep in &root.optional_dependencies {
        opt_map.insert(dep.alias.clone(), dep_to_json(dep));
    }

    let mut root_obj = Map::new();
    if let Some(ref name) = root.name {
        root_obj.insert("name".to_string(), json!(name));
    }
    if let Some(ref version) = root.version {
        root_obj.insert("version".to_string(), json!(version));
    }
    root_obj.insert("path".to_string(), json!(root.path));
    root_obj.insert("private".to_string(), json!(root.private.unwrap_or(false)));

    if !deps_map.is_empty() {
        root_obj.insert("dependencies".to_string(), Value::Object(deps_map));
    }
    if !dev_map.is_empty() {
        root_obj.insert("devDependencies".to_string(), Value::Object(dev_map));
    }
    if !opt_map.is_empty() {
        root_obj.insert("optionalDependencies".to_string(), Value::Object(opt_map));
    }

    serde_json::to_string_pretty(&json!([root_obj])).expect("serialize local list")
}

fn dep_to_json(dep: &DepNode) -> Value {
    let mut obj = Map::new();
    obj.insert("from".to_string(), json!(dep.name));
    obj.insert("version".to_string(), json!(dep.version));
    obj.insert("path".to_string(), json!(dep.path));

    if !dep.dependencies.is_empty() {
        let mut child_deps = Map::new();
        for child in &dep.dependencies {
            child_deps.insert(child.alias.clone(), dep_to_json(child));
        }
        obj.insert("dependencies".to_string(), Value::Object(child_deps));
    }

    Value::Object(obj)
}

fn render_local_parseable(root: &LocalTreeRoot, long: bool) -> String {
    let mut lines = Vec::new();

    let root_line = if long {
        let mut line = root.path.clone();
        if let Some(ref name) = root.name {
            line.push(':');
            line.push_str(name);
            if let Some(ref version) = root.version {
                line.push('@');
                line.push_str(version);
            }
            if root.private == Some(true) {
                line.push_str(":PRIVATE");
            }
        }
        line
    } else {
        root.path.clone()
    };
    lines.push(root_line);

    let mut seen = HashSet::new();
    seen.insert(root.path.clone());

    for dep in &root.dependencies {
        flatten_parseable(dep, &mut lines, &mut seen, long);
    }
    for dep in &root.dev_dependencies {
        flatten_parseable(dep, &mut lines, &mut seen, long);
    }
    for dep in &root.optional_dependencies {
        flatten_parseable(dep, &mut lines, &mut seen, long);
    }

    lines.join("\n")
}

fn flatten_parseable(
    dep: &DepNode,
    lines: &mut Vec<String>,
    seen: &mut HashSet<String>,
    long: bool,
) {
    if !seen.insert(dep.path.clone()) && !dep.path.is_empty() {
        return;
    }

    if long {
        if dep.alias != dep.name {
            if dep.version.contains('@') {
                lines.push(format!("{}:{} {}", dep.path, dep.alias, dep.version));
            } else {
                lines.push(format!("{}:{} npm:{}@{}", dep.path, dep.alias, dep.name, dep.version));
            }
        } else if dep.version.contains('@') {
            lines.push(format!("{}:{}", dep.path, dep.version));
        } else {
            lines.push(format!("{}:{}@{}", dep.path, dep.name, dep.version));
        }
    } else {
        lines.push(dep.path.clone());
    }

    for child in &dep.dependencies {
        flatten_parseable(child, lines, seen, long);
    }
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
#[path = "list/tests.rs"]
mod tests;
