use super::{
    DepType, HashMap, HashSet, ImporterComponents, IncludeFilter, Lockfile, PackageKey, Path,
    PathBuf, PeerSatisfactionEdges, SbomComponentType, SbomResult, SnapshotEntry, State,
    TransitiveEdges, WalkStores, build_purl, component_walk_context, confined_importer_dir,
    extract_author, extract_bugs_url, extract_description, extract_license, extract_repository,
    required_sbom_lockfile, safe_read_project_manifest_from_dir, walk_importer_components,
};

/// Every package's dependency type. A peer-satisfaction edge does not pass
/// the dependent's type on, whatever groups the run includes.
fn detect_dep_types(
    lockfile: &pnpm_lockfile::Lockfile,
    transitive: TransitiveEdges<'_>,
) -> HashMap<PackageKey, DepType> {
    let snapshots = lockfile.snapshots.as_ref();
    let mut dep_types: HashMap<PackageKey, DepType> = HashMap::new();
    let mut walked: HashSet<(PackageKey, bool)> = HashSet::new();

    let dev_keys =
        importer_roots(lockfile, |importer| std::slice::from_ref(&importer.dev_dependencies));
    let prod_keys =
        importer_roots(lockfile, |importer| std::slice::from_ref(&importer.dependencies));
    let optional_keys =
        importer_roots(lockfile, |importer| std::slice::from_ref(&importer.optional_dependencies));
    let prod_keys = [prod_keys, optional_keys].concat();

    detect_dep_types_walk(snapshots, &mut dep_types, &mut walked, &dev_keys, true, transitive);
    detect_dep_types_walk(snapshots, &mut dep_types, &mut walked, &prod_keys, false, transitive);
    dep_types
}

/// The snapshot keys every importer's chosen dependency maps resolve to.
fn importer_roots<'a>(
    lockfile: &'a pnpm_lockfile::Lockfile,
    maps: impl Fn(
        &'a pnpm_lockfile::ProjectSnapshot,
    ) -> &'a [Option<pnpm_lockfile::ResolvedDependencyMap>],
) -> Vec<PackageKey> {
    lockfile.importers
        .values()
        .flat_map(|importer| maps(importer).iter().flatten())
        .flatten()
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .collect()
}

fn detect_dep_types_walk(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    dep_types: &mut HashMap<PackageKey, DepType>,
    walked: &mut HashSet<(PackageKey, bool)>,
    initial_keys: &[PackageKey],
    is_dev: bool,
    transitive: TransitiveEdges<'_>,
) {
    let mut queue: Vec<PackageKey> = initial_keys.to_vec();

    while let Some(key) = queue.pop() {
        if !walked.insert((key.clone(), is_dev)) {
            continue;
        }
        record_dep_type(dep_types, &key, is_dev);

        let Some(snapshot) = snapshots.and_then(|snapshots| snapshots.get(&key)) else {
            continue;
        };

        queue.extend(transitive.children(&key, snapshot));
    }
}

/// Record what a package is reachable as. A package the dev walk already
/// recorded and the prod walk reaches too is production, not dev-only.
fn record_dep_type(dep_types: &mut HashMap<PackageKey, DepType>, key: &PackageKey, is_dev: bool) {
    if is_dev {
        dep_types.entry(key.clone()).or_insert(DepType::DevOnly);
        return;
    }
    if dep_types.get(key) == Some(&DepType::DevOnly) {
        dep_types.insert(key.clone(), DepType::ProdOnly);
        return;
    }
    dep_types.entry(key.clone()).or_insert(DepType::ProdOnly);
}

pub(super) fn collect_components(
    state: &State,
    include: IncludeFilter,
    sbom_type: SbomComponentType,
    exclude_peers: bool,
    lockfile_only: bool,
    filter_importer_ids: Option<&[&str]>,
    virtual_store_dirs_override: Option<&[PathBuf]>,
) -> miette::Result<SbomResult> {
    let lockfile = required_sbom_lockfile(state)?;

    let lockfile_dir = state.lockfile_dir().to_path_buf();
    let (manifest, root_manifest) = read_sbom_manifests(state, &lockfile_dir, filter_importer_ids);
    let root = RootMetadata::from_manifests(&manifest, root_manifest.as_ref());
    let peer_edges = PeerSatisfactionEdges::of_lockfile(lockfile, state.config.peer_edge_options());
    let dep_types = detect_dep_types(lockfile, TransitiveEdges::classifying(include, &peer_edges));

    let default_virtual_store_dirs = [state.config.effective_virtual_store_dir().to_path_buf()];
    let virtual_store_dirs = if lockfile_only {
        &[][..]
    } else {
        virtual_store_dirs_override.unwrap_or(&default_virtual_store_dirs)
    };

    let ctx = component_walk_context(
        state,
        lockfile,
        &dep_types,
        virtual_store_dirs,
        TransitiveEdges::walking(include, &peer_edges),
    );

    let mut stores = WalkStores {
        queue: initial_importer_ids(lockfile, filter_importer_ids),
        ..Default::default()
    };
    walk_importer_components(
        &ImporterComponents {
            lockfile,
            lockfile_dir: &lockfile_dir,
            include,
            exclude_peers,
            root_purl: &root.purl,
            ctx: &ctx,
        },
        &mut stores,
    );

    Ok(assemble_sbom_result(root, sbom_type, stores))
}

/// What the root manifest says about the SBOM's root component.
struct RootMetadata {
    name: String,
    version: String,
    license: Option<String>,
    description: Option<String>,
    author: Option<String>,
    repository: Option<String>,
    bugs_url: Option<String>,
    purl: String,
}

impl RootMetadata {
    pub(super) fn from_manifests(
        manifest: &serde_json::Value,
        root_manifest: Option<&serde_json::Value>,
    ) -> Self {
        let name = manifest
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let version = manifest
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0")
            .to_string();

        let resolve = |field_keys: &[&str], extract: fn(&serde_json::Value) -> Option<String>| {
            resolve_root_field(manifest, root_manifest, field_keys, extract)
        };

        RootMetadata {
            purl: build_purl(&name, &version),
            license: resolve(&["license", "licenses"], extract_license),
            description: resolve(&["description"], extract_description),
            author: resolve(&["author"], extract_author),
            repository: resolve(&["repository"], extract_repository),
            bugs_url: resolve(&["bugs"], extract_bugs_url),
            name,
            version,
        }
    }
}

/// The root component's value for one manifest field, read from the workspace
/// root manifest only when the filtered project declares none of `field_keys`.
///
/// A field the project declares stays the project's own even when the value
/// resolves to nothing: blank, `null`, or a form no SBOM may publish. The
/// workspace root's author, license, or repository would name the wrong party.
fn resolve_root_field(
    manifest: &serde_json::Value,
    root_manifest: Option<&serde_json::Value>,
    field_keys: &[&str],
    extract: fn(&serde_json::Value) -> Option<String>,
) -> Option<String> {
    let declared = field_keys
        .iter()
        .any(|key| manifest.get(key).is_some());
    if declared { extract(manifest) } else { root_manifest.and_then(extract) }
}

fn initial_importer_ids(lockfile: &Lockfile, filter_importer_ids: Option<&[&str]>) -> Vec<String> {
    lockfile.importers
        .keys()
        .filter(|id| filter_importer_ids.is_none_or(|ids| ids.contains(&id.as_str())))
        .cloned()
        .collect()
}

/// The manifest the SBOM's root component describes, and the workspace root
/// manifest it inherits absent fields from. Only a `--filter` that narrows the
/// run to one importer below the lockfile directory has the second: otherwise
/// the root component already describes the lockfile directory's own manifest.
///
/// An importer id is a raw lockfile key, so it is confined to the
/// workspace before it turns into an on-disk path: neither a crafted
/// `../foo` / absolute key nor a symlinked importer dir may read a
/// project manifest outside the workspace.
fn read_sbom_manifests(
    state: &State,
    lockfile_dir: &Path,
    filter_importer_ids: Option<&[&str]>,
) -> (serde_json::Value, Option<serde_json::Value>) {
    let fallback = |importer_id: &str| {
        if state.active_importer_id() == importer_id {
            state.manifest.value().clone()
        } else {
            serde_json::json!({})
        }
    };
    let root_manifest = safe_read_project_manifest_from_dir(lockfile_dir)
        .ok()
        .flatten()
        .unwrap_or_else(|| fallback("."));

    let single_id = match filter_importer_ids {
        Some(&[single_id]) if single_id != "." => single_id,
        _ => return (root_manifest, None),
    };

    let project_manifest = confined_importer_dir(lockfile_dir, single_id)
        .and_then(|dir| safe_read_project_manifest_from_dir(&dir).ok().flatten())
        .unwrap_or_else(|| fallback(single_id));

    (project_manifest, Some(root_manifest))
}

fn assemble_sbom_result(
    root: RootMetadata,
    sbom_type: SbomComponentType,
    stores: WalkStores,
) -> SbomResult {
    SbomResult {
        components: stores.components_map.into_values().collect(),
        relationships: stores.relationships,
        root_name: root.name,
        root_version: root.version,
        root_type: sbom_type,
        root_license: root.license,
        root_description: root.description,
        root_author: root.author,
        root_repository: root.repository,
        root_bugs_url: root.bugs_url,
    }
}
