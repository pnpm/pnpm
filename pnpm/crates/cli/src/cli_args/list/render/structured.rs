use super::{
    DependencyNode, HashSet, LongPkgInfo, Map, Path, ProjectHierarchy, Value, json, plain,
    read_long_pkg_info,
};

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
    let rendered = projects
        .iter()
        .map(|project| render_parseable_for_project(&mut dep_paths, project, opts))
        .filter(|out| !out.is_empty());
    rendered.collect::<Vec<_>>().join("\n")
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

pub(super) fn flatten<'a>(
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
