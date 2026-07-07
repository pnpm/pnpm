use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use clap::Args;
use miette::{Context, IntoDiagnostic};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::Config;
use pacquet_global::{ListReportAs, list_global_packages};
pub(crate) use pacquet_lockfile::PkgNameVerPeer;
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
use serde_json::{Map, Value, json};

use crate::cli_args::{
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    sanitize::sanitize,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RecursionLimit {
    ProjectsOnly,
    Levels(u32),
    Unlimited,
}

fn parse_depth(text: &str) -> Result<RecursionLimit, String> {
    if text.eq_ignore_ascii_case("Infinity") || text == "-1" {
        return Ok(if text == "-1" {
            RecursionLimit::ProjectsOnly
        } else {
            RecursionLimit::Unlimited
        });
    }
    let n: u32 = text
        .parse()
        .map_err(|_| format!("expected a non-negative integer, Infinity, or -1, got `{text}`"))?;
    Ok(RecursionLimit::Levels(n))
}

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

    #[clap(long, default_value = "0", value_parser = parse_depth, allow_hyphen_values = true)]
    pub depth: RecursionLimit,

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
    pub fn run(self, config: &Config, dir: &Path, recursive: bool) -> miette::Result<()> {
        if self.global {
            return self.run_global(config);
        }
        if recursive {
            return self.run_recursive(config, dir);
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

    fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        let (projects, _) = discover_workspace_projects(workspace_root)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;

        let roots = if self.depth == RecursionLimit::ProjectsOnly {
            selection
                .selected
                .values()
                .map(|node| project_only_root(node.package.project))
                .collect::<Vec<_>>()
        } else {
            let mut roots = Vec::new();
            for project_dir in selection.selected.keys() {
                if let LocalRootResult::Root(root) = self.local_root(config, project_dir)? {
                    roots.push(root);
                }
            }
            roots
        };

        self.print_roots(&roots);
        Ok(())
    }

    fn run_local(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        match self.local_root(config, dir)? {
            LocalRootResult::Root(root) => self.print_roots(&[root]),
            LocalRootResult::NoLockfile { lockfile_dir } => {
                if self.packages.is_empty() {
                    println!("No lockfile found in {}", lockfile_dir.display());
                } else {
                    println!("No matching packages found");
                }
            }
            LocalRootResult::NoPackages => {
                if self.packages.is_empty() {
                    println!("No packages found");
                } else {
                    println!("No matching packages found");
                }
            }
            LocalRootResult::NoMatchingPackages => println!("No matching packages found"),
        }
        Ok(())
    }

    fn local_root(&self, config: &Config, dir: &Path) -> miette::Result<LocalRootResult> {
        let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
        let lockfile_result = if self.lockfile_only {
            Lockfile::load_wanted_from_dir(lockfile_dir)
        } else {
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
                Ok(Some(lf)) => Ok(Some(lf)),
                Ok(None) => Lockfile::load_wanted_from_dir(lockfile_dir),
                Err(e) => Err(e),
            }
        };
        let Some(lockfile) = lockfile_result.into_diagnostic().wrap_err("load lockfile")? else {
            return Ok(LocalRootResult::NoLockfile { lockfile_dir: lockfile_dir.to_path_buf() });
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
            return Ok(LocalRootResult::NoPackages);
        };

        let manifest_path = dir.join("package.json");
        let (root_name, root_version, root_private) = read_root_manifest(&manifest_path)?;

        let has_both = self.production == self.dev;
        let include_prod = has_both || self.production;
        let include_dev = has_both || self.dev;
        let include_optional = !self.no_optional;

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
                return Ok(LocalRootResult::NoMatchingPackages);
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

        Ok(LocalRootResult::Root(root))
    }

    fn print_roots(&self, roots: &[LocalTreeRoot]) {
        if self.json {
            let output = match roots {
                [root] => render_local_json(root),
                _ => render_local_roots_json(roots),
            };
            println!("{output}");
        } else if self.parseable {
            let output = roots
                .iter()
                .map(|root| render_local_parseable(root, self.long))
                .filter(|output| !output.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if !output.is_empty() {
                println!("{output}");
            }
        } else {
            let joiner = if self.depth == RecursionLimit::ProjectsOnly { "\n" } else { "\n\n" };
            let output = roots
                .iter()
                .map(|root| render_local_tree(root, self.long))
                .filter(|output| !output.is_empty())
                .collect::<Vec<_>>()
                .join(joiner);
            if !output.is_empty() {
                println!("{output}");
            }
        }
    }
}

enum LocalRootResult {
    Root(LocalTreeRoot),
    NoLockfile { lockfile_dir: PathBuf },
    NoPackages,
    NoMatchingPackages,
}

#[derive(Debug, Clone)]
pub(crate) struct DepNode {
    pub(crate) alias: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) path: String,
    pub(crate) is_peer: bool,
    pub(crate) is_dev: bool,
    pub(crate) is_optional: bool,
    pub(crate) dependencies: Vec<DepNode>,
}

#[derive(Debug)]
pub(crate) struct LocalTreeRoot {
    pub(crate) name: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) private: Option<bool>,
    pub(crate) path: String,
    pub(crate) dependencies: Vec<DepNode>,
    pub(crate) dev_dependencies: Vec<DepNode>,
    pub(crate) optional_dependencies: Vec<DepNode>,
}

fn project_only_root(project: &pacquet_workspace::Project) -> LocalTreeRoot {
    let manifest = project.manifest.value();
    LocalTreeRoot {
        name: manifest.get("name").and_then(Value::as_str).map(str::to_string),
        version: manifest.get("version").and_then(Value::as_str).map(str::to_string),
        private: manifest.get("private").and_then(Value::as_bool),
        path: project.root_dir.to_string_lossy().into_owned(),
        dependencies: Vec::new(),
        dev_dependencies: Vec::new(),
        optional_dependencies: Vec::new(),
    }
}

struct BuiltTree {
    dependencies: Vec<DepNode>,
    dev_dependencies: Vec<DepNode>,
    optional_dependencies: Vec<DepNode>,
}

struct BuildContext<'a> {
    snapshots: Option<&'a HashMap<PkgNameVerPeer, SnapshotEntry>>,
    packages: Option<&'a HashMap<PkgNameVerPeer, pacquet_lockfile::PackageMetadata>>,
    depth: RecursionLimit,
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
    if ctx.depth == RecursionLimit::ProjectsOnly {
        return BuiltTree {
            dependencies: Vec::new(),
            dev_dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
        };
    }

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

            let children = match ctx.depth {
                RecursionLimit::Unlimited => resolve_snapshot_children(key, ctx, current_depth + 1),
                RecursionLimit::Levels(n) if (current_depth as u32) < n => {
                    resolve_snapshot_children(key, ctx, current_depth + 1)
                }
                _ => Vec::new(),
            };

            (dep_name, dep_version, peer, children)
        }
        None => (alias.clone(), version_str, false, Vec::new()),
    };

    let pkg_path = if let Some(key) = &snapshot_key {
        let vsn = key.to_virtual_store_name(ctx.virtual_store_dir_max_length);
        format!(".pnpm/{vsn}/node_modules/{resolved_name}")
    } else if let Some(link_target) = spec.version.as_link_target() {
        format!("link:{link_target}")
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

        let grandchildren = match ctx.depth {
            RecursionLimit::Unlimited => child_key
                .as_ref()
                .map_or(Vec::new(), |ck| resolve_snapshot_children(ck, ctx, current_depth + 1)),
            RecursionLimit::Levels(n) if (current_depth as u32) < n => child_key
                .as_ref()
                .map_or(Vec::new(), |ck| resolve_snapshot_children(ck, ctx, current_depth + 1)),
            _ => Vec::new(),
        };

        let pkg_path = if let Some(ck) = &child_key {
            let vsn = ck.to_virtual_store_name(ctx.virtual_store_dir_max_length);
            format!(".pnpm/{vsn}/node_modules/{child_name}")
        } else if let Some(link_target) = dep_ref.as_link_target() {
            format!("link:{link_target}")
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

pub(crate) fn get_peer_set(
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

pub(crate) fn sort_deps(deps: &mut [DepNode]) {
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    for dep in deps.iter_mut() {
        sort_deps(&mut dep.dependencies);
    }
}

pub(crate) fn matches_params(params: &[String], alias: &str) -> bool {
    if params.is_empty() {
        return true;
    }
    params.iter().any(|pattern| glob_match(pattern, alias))
}

pub(crate) fn dep_or_subtree_matches(dep: &DepNode, params: &[String]) -> bool {
    if params.is_empty() {
        return true;
    }
    if matches_params(params, &dep.alias) || matches_params(params, &dep.name) {
        return true;
    }
    dep.dependencies.iter().any(|child| dep_or_subtree_matches(child, params))
}

pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
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

pub(crate) fn read_root_manifest(
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

pub(crate) fn render_local_tree(root: &LocalTreeRoot, long: bool) -> String {
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

pub(crate) fn print_label(dep: &DepNode, color: &dyn Fn(&str) -> String) -> String {
    let alias = sanitize(&dep.alias);
    let name = sanitize(&dep.name);
    let version = sanitize(&dep.version);
    if alias == name {
        format!("{}{}", color(&name), gray(&format!("@{version}")))
    } else if version.contains('@') {
        format!("{}{}", color(&alias), gray(&format!("@{version}")))
    } else {
        format!("{}{}", color(&alias), gray(&format!("@npm:{name}@{version}")))
    }
}

pub(crate) fn name_at_version(name: &str, version: &str) -> String {
    let name = sanitize(name);
    if version.is_empty() {
        name.into_owned()
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

pub(crate) fn render_local_json(root: &LocalTreeRoot) -> String {
    render_local_roots_json(std::slice::from_ref(root))
}

fn render_local_roots_json(roots: &[LocalTreeRoot]) -> String {
    let root_objs = roots.iter().map(local_root_to_json).collect::<Vec<_>>();
    serde_json::to_string_pretty(&root_objs).expect("serialize local list")
}

fn local_root_to_json(root: &LocalTreeRoot) -> Value {
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

    Value::Object(root_obj)
}

pub(crate) fn dep_to_json(dep: &DepNode) -> Value {
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

pub(crate) fn render_local_parseable(root: &LocalTreeRoot, long: bool) -> String {
    let mut lines = Vec::new();

    let root_line = if long {
        let mut line = sanitize(&root.path).to_string();
        if let Some(ref name) = root.name {
            line.push(':');
            line.push_str(&sanitize(name));
            if let Some(ref version) = root.version {
                line.push('@');
                line.push_str(&sanitize(version));
            }
            if root.private == Some(true) {
                line.push_str(":PRIVATE");
            }
        }
        line
    } else {
        sanitize(&root.path).to_string()
    };
    lines.push(root_line);

    let mut seen = HashSet::new();
    seen.insert(sanitize(&root.path).to_string());

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
    let path = sanitize(&dep.path);
    if !seen.insert(path.to_string()) && !path.is_empty() {
        return;
    }

    if long {
        let alias = sanitize(&dep.alias);
        let name = sanitize(&dep.name);
        let version = sanitize(&dep.version);
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
        lines.push(path.to_string());
    }

    for child in &dep.dependencies {
        flatten_parseable(child, lines, seen, long);
    }
}

fn dim(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn bold(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn cyan_bright(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}

fn gray(text: &str) -> String {
    sanitize(text).if_supports_color(Stream::Stdout, |t| t.bright_black()).to_string()
}

#[cfg(test)]
mod tests;
