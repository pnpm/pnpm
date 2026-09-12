use super::{
    DepType, HashMap, HashSet, IncludeFilter, IndexMap, InstallabilityOptions, PackageKey,
    PackageMetadata, Path, PathBuf, PkgName, PkgNameVerPeer, SbomComponent, SbomRelationship,
    SnapshotEntry, State, build_purl, confined_importer_dir, extract_author, extract_bugs_url,
    extract_homepage, extract_repository, integrity_string, normalize_link_path,
    peer_names_from_manifest, platform_incompatible_optional, read_pkg_metadata_from_store,
    safe_read_package_json_from_dir, tarball_url_for_component,
};

pub(super) struct WalkContext<'a> {
    pub(super) snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub(super) packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub(super) dep_types: &'a HashMap<PackageKey, DepType>,
    pub(super) default_registry: &'a str,
    pub(super) virtual_store_dirs: &'a [PathBuf],
    pub(super) virtual_store_dir_max_length: usize,
    pub(super) include_optional_transitive: bool,
    pub(super) installability: InstallabilityOptions<'a>,
}

/// The collections the importer walk fills.
#[derive(Default)]
pub(super) struct WalkStores {
    pub(super) components_map: IndexMap<String, SbomComponent>,
    pub(super) relationships: Vec<SbomRelationship>,
    pub(super) visited: HashSet<PackageKey>,
    pub(super) ws_purl_by_importer: HashMap<String, String>,
    pub(super) queue: Vec<String>,
    pub(super) visited_importers: HashSet<String>,
}

/// What the importer walk accumulates.
struct ImporterWalk<'a> {
    components_map: &'a mut IndexMap<String, SbomComponent>,
    relationships: &'a mut Vec<SbomRelationship>,
    visited: &'a mut HashSet<PackageKey>,
    /// The component purl each workspace importer resolved to, so a
    /// linked sibling's own dependencies hang off it rather than off the
    /// root.
    ws_purl_by_importer: &'a mut HashMap<String, String>,
    queue: &'a mut Vec<String>,
}

/// The inputs every importer's collection reads.
pub(super) struct ImporterComponents<'a> {
    pub(super) lockfile: &'a pnpm_lockfile::Lockfile,
    pub(super) lockfile_dir: &'a Path,
    pub(super) include: &'a IncludeFilter,
    pub(super) exclude_peers: bool,
    pub(super) root_purl: &'a str,
    pub(super) ctx: &'a WalkContext<'a>,
}

fn collect_importer_components(
    inputs: &ImporterComponents<'_>,
    importer_id: &str,
    importer: &pnpm_lockfile::ProjectSnapshot,
    walk: &mut ImporterWalk<'_>,
) {
    let parent_purl = walk
        .ws_purl_by_importer
        .get(importer_id)
        .cloned()
        .unwrap_or_else(|| inputs.root_purl.to_owned());

    let importer_peer_names = importer_peer_names(inputs, importer_id);
    let (dev_dep_names, prod_dep_names) = importer_dependency_names(importer);

    let dep_maps = included_importer_dependencies(inputs.include, importer);
    for (name, spec) in dep_maps.into_iter().flatten().flatten() {
        if importer_peer_names.contains(&name.to_string()) {
            continue;
        }
        let dev_only = dev_dep_names.contains(&name.to_string())
            && !prod_dep_names.contains(&name.to_string());
        let linked = collect_linked_workspace_component(
            inputs,
            importer_id,
            name,
            spec,
            &parent_purl,
            dev_only,
            walk,
        );
        if linked {
            continue;
        }
        if let Some(snapshot_key) = spec.version.resolved_key(name) {
            walk_snapshot(
                &snapshot_key,
                &parent_purl,
                inputs.ctx,
                walk.components_map,
                walk.relationships,
                walk.visited,
            );
        }
    }
}

/// The peer dependency names the importer's own manifest declares, when
/// peers are left out of the SBOM.
fn importer_peer_names(inputs: &ImporterComponents<'_>, importer_id: &str) -> HashSet<String> {
    if !inputs.exclude_peers {
        return HashSet::new();
    }
    confined_importer_dir(inputs.lockfile_dir, importer_id)
        .and_then(|dir| safe_read_package_json_from_dir(&dir).ok().flatten())
        .map(|manifest| peer_names_from_manifest(&manifest))
        .unwrap_or_default()
}

/// Record a `link:` dependency that resolves to another workspace
/// importer as a component of its own, and queue that importer so its
/// dependencies hang off it. `false` when the dependency is not one.
fn collect_linked_workspace_component(
    inputs: &ImporterComponents<'_>,
    importer_id: &str,
    name: &PkgName,
    spec: &pnpm_lockfile::ResolvedDependencySpec,
    parent_purl: &str,
    dev_only: bool,
    walk: &mut ImporterWalk<'_>,
) -> bool {
    let Some(link_target) = spec.version.as_link_target() else { return false };
    let Some(target_id) = normalize_link_path(importer_id, link_target) else { return false };
    if !inputs.lockfile.importers.contains_key(target_id.as_str()) {
        return false;
    }
    let Some(ws_dir) = confined_importer_dir(inputs.lockfile_dir, &target_id) else {
        return false;
    };
    let Ok(Some(ws_manifest)) = safe_read_package_json_from_dir(&ws_dir) else {
        return false;
    };
    let ws_name = ws_manifest
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or(&name.to_string())
        .to_string();
    let ws_version =
        ws_manifest.get("version").and_then(|value| value.as_str()).unwrap_or("0.0.0").to_string();
    let ws_purl = build_purl(&ws_name, &ws_version);
    walk.relationships.push(SbomRelationship { from: parent_purl.to_owned(), to: ws_purl.clone() });
    // A sibling reached both ways is a production dependency.
    if let Some(existing) = walk.components_map.get_mut(&ws_purl) {
        if !dev_only && existing.dep_type == DepType::DevOnly {
            existing.dep_type = DepType::ProdOnly;
        }
    } else {
        walk.components_map.insert(
            ws_purl.clone(),
            workspace_component(&ws_manifest, ws_name, ws_version, dev_only),
        );
    }
    walk.ws_purl_by_importer.insert(target_id.clone(), ws_purl);
    walk.queue.push(target_id);
    true
}

/// The SBOM component describing a linked workspace project.
fn workspace_component(
    ws_manifest: &serde_json::Value,
    name: String,
    version: String,
    dev_only: bool,
) -> SbomComponent {
    SbomComponent {
        purl: build_purl(&name, &version),
        name,
        version,
        dep_type: if dev_only { DepType::DevOnly } else { DepType::ProdOnly },
        integrity: None,
        tarball_url: None,
        license: ws_manifest
            .get("license")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        description: ws_manifest
            .get("description")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        author: extract_author(ws_manifest),
        homepage: extract_homepage(ws_manifest),
        repository: extract_repository(ws_manifest),
        bugs_url: extract_bugs_url(ws_manifest),
    }
}

fn walk_snapshot(
    initial_key: &PkgNameVerPeer,
    initial_parent_purl: &str,
    ctx: &WalkContext<'_>,
    components_map: &mut IndexMap<String, SbomComponent>,
    relationships: &mut Vec<SbomRelationship>,
    visited: &mut HashSet<PackageKey>,
) {
    let mut queue: Vec<(PkgNameVerPeer, String)> =
        vec![(initial_key.clone(), initial_parent_purl.to_string())];

    while let Some((key, parent_purl)) = queue.pop() {
        let name = key.name.to_string();
        let pkg_meta = ctx.packages.and_then(|pkgs| pkgs.get(&key.without_peer()));
        let version = pkg_meta
            .and_then(|meta| meta.version.clone())
            .unwrap_or_else(|| key.suffix.version().to_string());

        if skipped_optional_package(&key, pkg_meta, ctx) {
            continue;
        }

        let purl = build_purl(&name, &version);

        relationships.push(SbomRelationship { from: parent_purl, to: purl.clone() });

        if !visited.insert(key.clone()) {
            continue;
        }

        if !components_map.contains_key(&purl) {
            components_map.insert(purl.clone(), snapshot_component(&key, name, version, ctx));
        }

        let Some(snapshot) = ctx.snapshots.and_then(|snapshots| snapshots.get(&key)) else {
            continue;
        };
        queue.extend(snapshot_children(snapshot, ctx).map(|child| (child, purl.clone())));
    }
}

/// Whether the package is an optional dependency this host cannot
/// install, and so is not part of the installed set the SBOM describes.
///
/// `virtual_store_dirs` is empty under `--lockfile-only`, which
/// describes the whole lockfile graph, platform-independently.
fn skipped_optional_package(
    key: &PkgNameVerPeer,
    pkg_meta: Option<&pnpm_lockfile::PackageMetadata>,
    ctx: &WalkContext<'_>,
) -> bool {
    if ctx.virtual_store_dirs.is_empty() {
        return false;
    }
    let optional = ctx
        .snapshots
        .is_some_and(|snapshots| snapshots.get(key).is_some_and(|snapshot| snapshot.optional));
    platform_incompatible_optional(&key.name.bare, optional, pkg_meta, &ctx.installability)
}

/// The SBOM component describing one installed package, with whatever
/// metadata its store copy carries.
fn snapshot_component(
    key: &PkgNameVerPeer,
    name: String,
    version: String,
    ctx: &WalkContext<'_>,
) -> SbomComponent {
    let pkg_meta = ctx.packages.and_then(|packages| packages.get(&key.without_peer()));
    let store_meta = read_pkg_metadata_from_store(key, &name, ctx);
    SbomComponent {
        purl: build_purl(&name, &version),
        integrity: pkg_meta.and_then(|meta| integrity_string(&meta.resolution)),
        tarball_url: pkg_meta.and_then(|meta| {
            tarball_url_for_component(&meta.resolution, &name, &version, ctx.default_registry)
        }),
        name,
        version,
        dep_type: ctx.dep_types.get(key).copied().unwrap_or(DepType::ProdOnly),
        license: store_meta.license,
        description: store_meta.description,
        author: store_meta.author,
        homepage: store_meta.homepage,
        repository: store_meta.repository,
        bugs_url: store_meta.bugs_url,
    }
}

/// The snapshots one package depends on, including its optional ones
/// when the run describes those too.
fn snapshot_children<'a>(
    snapshot: &'a SnapshotEntry,
    ctx: &WalkContext<'_>,
) -> impl Iterator<Item = PkgNameVerPeer> + 'a {
    let optional_iter = ctx
        .include_optional_transitive
        .then(|| snapshot.optional_dependencies.iter().flatten())
        .into_iter()
        .flatten();
    snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(optional_iter)
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
}

pub(super) fn walk_importer_components(inputs: &ImporterComponents<'_>, stores: &mut WalkStores) {
    let mut walk = ImporterWalk {
        components_map: &mut stores.components_map,
        relationships: &mut stores.relationships,
        visited: &mut stores.visited,
        ws_purl_by_importer: &mut stores.ws_purl_by_importer,
        queue: &mut stores.queue,
    };
    while let Some(importer_id) = walk.queue.pop() {
        if !stores.visited_importers.insert(importer_id.clone()) {
            continue;
        }
        let Some(importer) = inputs.lockfile.importers.get(importer_id.as_str()) else {
            continue;
        };
        collect_importer_components(inputs, &importer_id, importer, &mut walk);
    }
}

pub(super) fn component_walk_context<'a>(
    state: &'a State,
    lockfile: &'a pnpm_lockfile::Lockfile,
    dep_types: &'a HashMap<pnpm_lockfile::PackageKey, DepType>,
    virtual_store_dirs: &'a [PathBuf],
    include_optional_transitive: bool,
) -> WalkContext<'a> {
    WalkContext {
        snapshots: lockfile.snapshots.as_ref(),
        packages: lockfile.packages.as_ref(),
        dep_types,
        default_registry: &state.config.registry,
        virtual_store_dirs,
        virtual_store_dir_max_length: state.config.virtual_store_dir_max_length as usize,
        include_optional_transitive,
        installability: InstallabilityOptions {
            supported_architectures: state.config.supported_architectures.as_ref(),
            current_os: pnpm_detect_libc::host_platform(),
            current_cpu: pnpm_detect_libc::host_arch(),
            current_libc: pnpm_graph_hasher::host_libc(),
            ..Default::default()
        },
    }
}

fn importer_dependency_names(
    importer: &pnpm_lockfile::ProjectSnapshot,
) -> (HashSet<String>, HashSet<String>) {
    let dev_dep_names: HashSet<String> = importer
        .dev_dependencies
        .as_ref()
        .map(|deps| deps.keys().map(ToString::to_string).collect())
        .unwrap_or_default();
    let prod_dep_names: HashSet<String> = importer
        .dependencies
        .iter()
        .chain(importer.optional_dependencies.iter())
        .flat_map(|deps| deps.keys())
        .map(ToString::to_string)
        .collect();

    (dev_dep_names, prod_dep_names)
}

fn included_importer_dependencies<'a>(
    include: &IncludeFilter,
    importer: &'a pnpm_lockfile::ProjectSnapshot,
) -> [Option<&'a pnpm_lockfile::ResolvedDependencyMap>; 3] {
    [
        include.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
        include.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
        include.optional_dependencies.then_some(importer.optional_dependencies.as_ref()).flatten(),
    ]
}
