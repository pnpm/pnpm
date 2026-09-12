use super::{
    DepType, HashMap, HashSet, ImporterComponents, IncludeFilter, Lockfile, PackageKey, Path,
    PathBuf, SbomComponentType, SbomResult, SnapshotEntry, State, WalkStores, build_purl,
    component_walk_context, confined_importer_dir, extract_author, extract_bugs_url,
    extract_repository, required_sbom_lockfile, safe_read_package_json_from_dir,
    walk_importer_components,
};

fn detect_dep_types(
    lockfile: &pnpm_lockfile::Lockfile,
    include_optional_transitive: bool,
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

    detect_dep_types_walk(
        snapshots,
        &mut dep_types,
        &mut walked,
        &dev_keys,
        true,
        include_optional_transitive,
    );
    detect_dep_types_walk(
        snapshots,
        &mut dep_types,
        &mut walked,
        &prod_keys,
        false,
        include_optional_transitive,
    );
    dep_types
}

/// The snapshot keys every importer's chosen dependency maps resolve to.
fn importer_roots<'a>(
    lockfile: &'a pnpm_lockfile::Lockfile,
    maps: impl Fn(
        &'a pnpm_lockfile::ProjectSnapshot,
    ) -> &'a [Option<pnpm_lockfile::ResolvedDependencyMap>],
) -> Vec<PackageKey> {
    let declared =
        lockfile.importers.values().flat_map(|importer| maps(importer).iter().flatten()).flatten();
    declared.filter_map(|(name, spec)| spec.version.resolved_key(name)).collect()
}

fn detect_dep_types_walk(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    dep_types: &mut HashMap<PackageKey, DepType>,
    walked: &mut HashSet<(PackageKey, bool)>,
    initial_keys: &[PackageKey],
    is_dev: bool,
    include_optional_transitive: bool,
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

        let optional_iter = include_optional_transitive
            .then(|| snapshot.optional_dependencies.iter().flatten())
            .into_iter()
            .flatten();
        for (alias, dep_ref) in snapshot.dependencies.iter().flatten().chain(optional_iter) {
            if let Some(child_key) = dep_ref.resolve(alias) {
                queue.push(child_key);
            }
        }
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
    include: &IncludeFilter,
    sbom_type: SbomComponentType,
    exclude_peers: bool,
    lockfile_only: bool,
    filter_importer_ids: Option<&[&str]>,
    virtual_store_dirs_override: Option<&[PathBuf]>,
) -> miette::Result<SbomResult> {
    let lockfile = required_sbom_lockfile(state)?;

    let lockfile_dir = state.lockfile_dir().to_path_buf();
    let root = RootMetadata::of(&read_root_manifest(state, &lockfile_dir, filter_importer_ids));
    let dep_types = detect_dep_types(lockfile, include.optional_dependencies);

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
        include.optional_dependencies,
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
    pub(super) fn of(manifest: &serde_json::Value) -> Self {
        let name = manifest.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let version =
            manifest.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0").to_string();
        RootMetadata {
            purl: build_purl(&name, &version),
            license: manifest.get("license").and_then(|v| v.as_str()).map(ToString::to_string),
            description: manifest
                .get("description")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            author: extract_author(manifest),
            repository: extract_repository(manifest),
            bugs_url: extract_bugs_url(manifest),
            name,
            version,
        }
    }
}

fn initial_importer_ids(lockfile: &Lockfile, filter_importer_ids: Option<&[&str]>) -> Vec<String> {
    lockfile
        .importers
        .keys()
        .filter(|id| filter_importer_ids.is_none_or(|ids| ids.contains(&id.as_str())))
        .cloned()
        .collect()
}

/// The manifest the SBOM's root component describes: the single filtered
/// importer's, or the lockfile directory's.
///
/// An importer id is a raw lockfile key, so it is confined to the
/// workspace before it turns into an on-disk path: neither a crafted
/// `../foo` / absolute key nor a symlinked importer dir may read a
/// `package.json` outside the workspace.
fn read_root_manifest(
    state: &State,
    lockfile_dir: &Path,
    filter_importer_ids: Option<&[&str]>,
) -> serde_json::Value {
    let fallback = |importer_id: &str| {
        if state.active_importer_id() == importer_id {
            state.manifest.value().clone()
        } else {
            serde_json::json!({})
        }
    };
    let Some(&[single_id]) = filter_importer_ids else {
        return safe_read_package_json_from_dir(lockfile_dir)
            .ok()
            .flatten()
            .unwrap_or_else(|| fallback("."));
    };
    confined_importer_dir(lockfile_dir, single_id)
        .and_then(|dir| safe_read_package_json_from_dir(&dir).ok().flatten())
        .unwrap_or_else(|| fallback(single_id))
}

fn assemble_sbom_result(
    root: RootMetadata,
    sbom_type: SbomComponentType,
    stores: WalkStores,
) -> SbomResult {
    SbomResult {
        root_name: root.name,
        root_version: root.version,
        root_type: sbom_type,
        root_license: root.license,
        root_description: root.description,
        root_author: root.author,
        root_repository: root.repository,
        root_bugs_url: root.bugs_url,
        components: stores.components_map.into_values().collect(),
        relationships: stores.relationships,
    }
}
