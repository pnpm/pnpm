use super::{
    ANY_VERSION_RANGE, CreateProjectsGraphOptions, GraphPkg, HashMap, HashSet, PackageManifest,
    PathBuf, Project, ProjectGraph, StageApprovalItem, StageContext, StageError, Value, Version,
    create_projects_graph, fetch_stage_tarball, is_any_version_range, is_valid_semver_range,
    read_tarball_manifest, sequence_graph,
};

/// The dependency order derived from the exact tarballs being approved.
pub(super) struct StageApprovalOrder {
    pub(super) dependency_stage_ids: HashMap<String, Vec<String>>,
    pub(super) order_indices: HashMap<String, usize>,
    pub(super) package_names: HashMap<String, String>,
}

/// Download every selected package before approval and derive the graph from
/// the package.json files that will reach the registry.
pub(super) async fn read_stage_approval_order(
    context: &StageContext,
    items: &[StageApprovalItem],
) -> miette::Result<StageApprovalOrder> {
    let mut projects = Vec::with_capacity(items.len());
    let mut stage_id_by_package_version: HashMap<(String, String), String> = HashMap::new();
    for item in items {
        projects.push(staged_project(context, item, &mut stage_id_by_package_version).await?);
    }
    let graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(true),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph;
    Ok(approval_order(&graph))
}

/// The stage's package as a workspace project, refusing a second stage of
/// the same package version.
async fn staged_project(
    context: &StageContext,
    item: &StageApprovalItem,
    stage_id_by_package_version: &mut HashMap<(String, String), String>,
) -> miette::Result<Project> {
    let root_dir = PathBuf::from(&item.id);
    let tarball = fetch_stage_tarball(context, &item.id).await?;
    let manifest = read_tarball_manifest(&tarball)?;
    let package_name = manifest
        .get("name")
        .and_then(Value::as_str)
        .ok_or(StageError::TarballManifestNotFound)?
        .to_owned();
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .ok_or(StageError::TarballManifestNotFound)?
        .to_owned();
    if let Some(first_stage_id) =
        stage_id_by_package_version.insert((package_name.clone(), version.clone()), item.id.clone())
    {
        return Err(StageError::DuplicateStagePackage {
            first_stage_id,
            second_stage_id: item.id.clone(),
            package_name,
            version,
        }
        .into());
    }
    let manifest = manifest_for_graph(manifest);
    Ok(Project {
        manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
        root_dir,
        dependency_manifest: None,
    })
}

fn approval_order(graph: &ProjectGraph<GraphPkg<'_>>) -> StageApprovalOrder {
    let mut dependency_stage_ids_by_stage_id = HashMap::new();
    let mut order_index_by_stage_id = HashMap::new();
    let mut package_name_by_stage_id = HashMap::new();
    for (order_index, root_dir) in sequence_graph(graph, graph).order.into_iter().enumerate() {
        let stage_id = root_dir.to_string_lossy().into_owned();
        order_index_by_stage_id.insert(stage_id.clone(), order_index);
        dependency_stage_ids_by_stage_id.insert(
            stage_id.clone(),
            graph[&root_dir]
                .dependencies
                .iter()
                .map(|dependency| dependency.to_string_lossy().into_owned())
                .collect(),
        );
        if let Some(package_name) =
            graph[&root_dir].package.project.manifest.value().get("name").and_then(Value::as_str)
        {
            package_name_by_stage_id.insert(stage_id, package_name.to_owned());
        }
    }
    StageApprovalOrder {
        dependency_stage_ids: dependency_stage_ids_by_stage_id,
        order_indices: order_index_by_stage_id,
        package_names: package_name_by_stage_id,
    }
}

/// Approve staged dependencies before the selected packages that need them.
pub(super) fn sort_items_for_approval(
    mut items: Vec<StageApprovalItem>,
    order: &StageApprovalOrder,
) -> Vec<StageApprovalItem> {
    items.sort_by_key(|item| order_index_of(item, order));
    items
}

/// Selected staged dependencies of `item` whose approval failed or was skipped.
pub(super) fn unavailable_dependencies(
    item: &StageApprovalItem,
    unpublished_stage_ids: &HashSet<String>,
    order: &StageApprovalOrder,
) -> Vec<String> {
    let dependencies = order.dependency_stage_ids.get(&item.id).into_iter().flatten();
    dependencies
        .filter(|stage_id| unpublished_stage_ids.contains(*stage_id))
        .map(|stage_id| {
            order.package_names.get(stage_id).cloned().unwrap_or_else(|| stage_id.clone())
        })
        .collect()
}

fn order_index_of(item: &StageApprovalItem, order: &StageApprovalOrder) -> usize {
    order.order_indices.get(&item.id).copied().unwrap_or(usize::MAX)
}

pub(super) fn manifest_for_graph(mut manifest: Value) -> Value {
    for field in ["peerDependencies", "devDependencies", "optionalDependencies", "dependencies"] {
        let Some(dependencies) = manifest.get(field).and_then(Value::as_object) else {
            continue;
        };
        let normalized = dependencies
            .iter()
            .filter_map(|(name, spec)| {
                let spec = spec.as_str()?;
                let (registry_name, registry_spec) =
                    PackageManifest::resolve_registry_dependency(name, spec);
                let (name, spec) =
                    if let Some(registry_spec) = registry_spec_for_graph(registry_spec) {
                        (registry_name, registry_spec)
                    } else {
                        (name.as_str(), spec)
                    };
                Some((name.to_owned(), Value::String(spec.to_owned())))
            })
            .collect();
        manifest[field] = Value::Object(normalized);
    }
    manifest
}

fn registry_spec_for_graph(spec: &str) -> Option<&str> {
    if Version::parse(spec).is_ok() {
        return Some(spec);
    }
    if !is_valid_semver_range(spec) {
        return None;
    }
    Some(if is_any_version_range(spec) { ANY_VERSION_RANGE } else { spec })
}
