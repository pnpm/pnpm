//! `pacquet sbom` — generate a Software Bill of Materials.
//!
//! Ports pnpm's `sbom` command
//! (`pnpm11/deps/compliance/commands/src/sbom/sbom.ts`).

use crate::{
    State,
    cli_args::{
        install::resolve_bool_override,
        recursive::{
            AutoExcludeRoot, discover_workspace_projects, no_projects_matched_message,
            notice_workspace_dir, select_recursive_projects, selected_importer_ids,
        },
    },
};
use clap::Args;
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_lockfile::{
    LazyLockfile, Lockfile, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    PkgNameVerPeer, SnapshotEntry,
};
use pnpm_package_is_installable::{
    InstallabilityOptions, WantedPlatformRef, platform_is_supported_with_inference,
};
use pnpm_package_manager::{importer_root_dir, validate_importer_id};
use pnpm_package_manifest::{extract_author, extract_homepage, safe_read_package_json_from_dir};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Display,
    hash::Hash,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SbomFormat {
    #[clap(name = "cyclonedx")]
    CycloneDx,
    #[clap(name = "spdx")]
    Spdx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SbomComponentType {
    Library,
    Application,
}

#[derive(Debug, Args)]
pub struct SbomArgs {
    /// The SBOM output format (required).
    #[clap(long = "sbom-format", value_enum)]
    pub format: SbomFormat,

    /// The component type for the root package (default: library).
    #[clap(long = "sbom-type", value_enum, default_value = "library")]
    pub sbom_type: SbomComponentType,

    /// The `CycloneDX` specification version (`1.5`, `1.6`, or `1.7`; default: `1.7`).
    /// Only valid with `--sbom-format cyclonedx`.
    #[clap(long = "sbom-spec-version")]
    pub spec_version: Option<String>,

    /// Only use lockfile data (skip reading from the store).
    #[clap(long)]
    pub lockfile_only: bool,

    /// Comma-separated list of SBOM authors (`CycloneDX` `metadata.authors`).
    #[clap(long = "sbom-authors")]
    pub authors: Option<String>,

    /// SBOM supplier name (`CycloneDX` `metadata.supplier`).
    #[clap(long = "sbom-supplier")]
    pub supplier: Option<String>,

    /// Only include production dependencies.
    #[clap(long, short = 'P', visible_alias = "production")]
    pub prod: bool,

    /// Only include dev dependencies.
    #[clap(long, short = 'D')]
    pub dev: bool,

    /// Exclude optional dependencies.
    #[clap(long = "no-optional", overrides_with = "optional")]
    pub no_optional: bool,

    /// Include optional dependencies.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,

    /// Exclude peer dependencies.
    #[clap(long = "exclude-peers")]
    pub exclude_peers: bool,

    /// Write SBOM to a file instead of stdout. Use `%s` for the
    /// package name and `%v` for the version.
    #[clap(long)]
    pub out: Option<String>,

    /// Generate a separate SBOM for each matched workspace package.
    #[clap(long)]
    pub split: bool,
}

struct IncludeFilter {
    dependencies: bool,
    dev_dependencies: bool,
    optional_dependencies: bool,
}

impl SbomArgs {
    fn include_filter(&self, include_optional: bool) -> IncludeFilter {
        IncludeFilter {
            dependencies: !self.dev,
            dev_dependencies: !self.prod,
            // pnpm's config reader clears `optional` for a dev-only run,
            // and leaves it alone for a production-only one.
            optional_dependencies: !self.dev
                && resolve_bool_override(self.optional, self.no_optional, include_optional),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepType {
    DevOnly,
    ProdOnly,
}

struct SbomComponent {
    name: String,
    version: String,
    purl: String,
    dep_type: DepType,
    integrity: Option<String>,
    tarball_url: Option<String>,
    license: Option<String>,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    bugs_url: Option<String>,
}

struct WalkContext<'a> {
    snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    dep_types: &'a HashMap<PackageKey, DepType>,
    default_registry: &'a str,
    virtual_store_dirs: &'a [PathBuf],
    virtual_store_dir_max_length: usize,
    include_optional_transitive: bool,
    installability: InstallabilityOptions<'a>,
}

struct SbomRelationship {
    from: String,
    to: String,
}

struct SbomResult {
    root_name: String,
    root_version: String,
    root_type: SbomComponentType,
    root_license: Option<String>,
    root_description: Option<String>,
    root_author: Option<String>,
    root_repository: Option<String>,
    root_bugs_url: Option<String>,
    components: Vec<SbomComponent>,
    relationships: Vec<SbomRelationship>,
}

/// Resolve a lockfile importer key to the on-disk directory whose
/// `package.json` the SBOM reads, returning `None` when that directory does
/// not stay inside the lockfile dir. Mirrors pnpm's SBOM importer handling
/// (`sbom.ts`): `validate_importer_id` is the cheap lexical pre-filter, then
/// both the lockfile dir and the importer dir are canonicalized so a
/// *symlinked* importer directory can't resolve outside the workspace, and a
/// resolved path outside the root is skipped rather than read. `importer_id`
/// comes from an untrusted lockfile.
fn confined_importer_dir(lockfile_dir: &Path, importer_id: &str) -> Option<PathBuf> {
    if validate_importer_id(importer_id).is_err() {
        return None;
    }
    let lockfile_root = std::fs::canonicalize(lockfile_dir).ok()?;
    let importer_dir = std::fs::canonicalize(importer_root_dir(lockfile_dir, importer_id)).ok()?;
    importer_dir.starts_with(&lockfile_root).then_some(importer_dir)
}

fn extract_repository(manifest: &serde_json::Value) -> Option<String> {
    let repo = manifest.get("repository")?;
    if let Some(s) = repo.as_str() {
        return Some(s.to_string());
    }
    repo.get("url").and_then(|u| u.as_str()).map(ToString::to_string)
}

fn strip_url_credentials(url: &str) -> String {
    if let Some(after_scheme) = url.find("://") {
        let scheme = &url[..after_scheme + 3];
        let rest = &url[after_scheme + 3..];
        if let Some(at_pos) = rest.find('@') {
            let after_host_start = &rest[at_pos + 1..];
            return format!("{scheme}{after_host_start}");
        }
    }
    url.to_string()
}

fn extract_bugs_url(manifest: &serde_json::Value) -> Option<String> {
    let bugs = manifest.get("bugs")?;
    let url = if let Some(s) = bugs.as_str() {
        s.to_string()
    } else {
        bugs.get("url")?.as_str()?.to_string()
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    Some(strip_url_credentials(&url))
}

fn registry_tarball_url(registry: &str, name: &str, version: &str) -> String {
    let registry = registry.trim_end_matches('/');
    let basename = name.rsplit('/').next().unwrap_or(name);
    format!("{registry}/{name}/-/{basename}-{version}.tgz")
}

fn tarball_url_for_component(
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
    registry: &str,
) -> Option<String> {
    match resolution {
        LockfileResolution::Registry(_) => Some(registry_tarball_url(registry, name, version)),
        LockfileResolution::Tarball(r) => Some(r.tarball.clone()),
        LockfileResolution::Git(r) => {
            let needs_prefix = r.repo.contains("://") && !r.repo.starts_with("git+");
            let prefix = if needs_prefix { "git+" } else { "" };
            Some(format!("{prefix}{}#{}", r.repo, r.commit))
        }
        _ => None,
    }
}

fn encode_purl_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix('@') { format!("%40{rest}") } else { name.to_string() }
}

fn build_purl(name: &str, version: &str) -> String {
    format!("pkg:npm/{}@{}", encode_purl_name(name), version)
}

fn is_simple_spdx_id(license: &str) -> bool {
    !license.is_empty()
        && license
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '+')
}

fn classify_license(license: &str) -> serde_json::Value {
    let is_expression =
        license.split_whitespace().any(|word| word == "AND" || word == "OR" || word == "WITH");
    if is_expression {
        serde_json::json!({ "expression": license })
    } else if is_simple_spdx_id(license) {
        serde_json::json!({ "license": { "id": license } })
    } else {
        serde_json::json!({ "license": { "name": license } })
    }
}

/// The resolution's integrity, but only where pnpm checks the downloaded
/// bytes against it — so an SBOM never publishes a checksum as an assurance
/// pnpm did not make. A git resolution's recorded hash is not one: nothing
/// verifies a checkout against it (see
/// [`pnpm_lockfile::GitResolution::integrity`]).
fn integrity_string(resolution: &LockfileResolution) -> Option<String> {
    match resolution {
        LockfileResolution::Registry(r) => Some(r.integrity.to_string()),
        LockfileResolution::Tarball(r) => r.integrity.as_ref().map(ToString::to_string),
        LockfileResolution::Binary(r) => Some(r.integrity.to_string()),
        _ => None,
    }
}

fn peer_names_from_manifest(manifest: &serde_json::Value) -> HashSet<String> {
    let regular: HashSet<&str> = ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .flat_map(|field| {
            manifest
                .get(field)
                .and_then(|v| v.as_object())
                .into_iter()
                .flat_map(|obj| obj.keys().map(String::as_str))
        })
        .collect();

    manifest
        .get("peerDependencies")
        .and_then(|v| v.as_object())
        .into_iter()
        .flat_map(|obj| obj.keys())
        .filter(|name| !regular.contains(name.as_str()))
        .cloned()
        .collect()
}

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
    lockfile
        .importers
        .values()
        .flat_map(|importer| maps(importer).iter().flatten())
        .flatten()
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .collect()
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

fn collect_components(
    state: &State,
    include: &IncludeFilter,
    sbom_type: SbomComponentType,
    exclude_peers: bool,
    lockfile_only: bool,
    filter_importer_ids: Option<&[&str]>,
    virtual_store_dirs_override: Option<&[PathBuf]>,
) -> miette::Result<SbomResult> {
    let lockfile = state
        .lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

    let Some(lockfile) = lockfile else {
        return Err(miette::miette!(
            code = "ERR_PNPM_SBOM_NO_LOCKFILE",
            "No pnpm-lock.yaml found: cannot generate SBOM without a lockfile"
        ));
    };

    let lockfile_dir = state.lockfile_dir().to_path_buf();

    let manifest_value = read_root_manifest(state, &lockfile_dir, filter_importer_ids);
    let root_name =
        manifest_value.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let root_version =
        manifest_value.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0").to_string();
    let root_license =
        manifest_value.get("license").and_then(|v| v.as_str()).map(ToString::to_string);
    let root_description =
        manifest_value.get("description").and_then(|v| v.as_str()).map(ToString::to_string);
    let root_author = extract_author(&manifest_value);
    let root_repository = extract_repository(&manifest_value);
    let root_bugs_url = extract_bugs_url(&manifest_value);

    let root_purl = build_purl(&root_name, &root_version);

    let dep_types = detect_dep_types(lockfile, include.optional_dependencies);

    let default_virtual_store_dirs = [state.config.effective_virtual_store_dir().to_path_buf()];
    let virtual_store_dirs = if lockfile_only {
        &[][..]
    } else {
        virtual_store_dirs_override.unwrap_or(&default_virtual_store_dirs)
    };

    let ctx = WalkContext {
        snapshots: lockfile.snapshots.as_ref(),
        packages: lockfile.packages.as_ref(),
        dep_types: &dep_types,
        default_registry: &state.config.registry,
        virtual_store_dirs,
        virtual_store_dir_max_length: state.config.virtual_store_dir_max_length as usize,
        include_optional_transitive: include.optional_dependencies,
        installability: InstallabilityOptions {
            supported_architectures: state.config.supported_architectures.as_ref(),
            current_os: pnpm_detect_libc::host_platform(),
            current_cpu: pnpm_detect_libc::host_arch(),
            current_libc: pnpm_graph_hasher::host_libc(),
            ..Default::default()
        },
    };

    let mut components_map: IndexMap<String, SbomComponent> = IndexMap::new();
    let mut relationships: Vec<SbomRelationship> = Vec::new();
    let mut visited: HashSet<PackageKey> = HashSet::new();
    let mut ws_purl_by_importer: HashMap<String, String> = HashMap::new();

    let initial_importer_ids: Vec<String> = lockfile
        .importers
        .keys()
        .filter(|id| filter_importer_ids.is_none_or(|ids| ids.contains(&id.as_str())))
        .cloned()
        .collect();

    let mut importer_queue: Vec<String> = initial_importer_ids;
    let mut visited_importers: HashSet<String> = HashSet::new();

    let mut walk = ImporterWalk {
        components_map: &mut components_map,
        relationships: &mut relationships,
        visited: &mut visited,
        ws_purl_by_importer: &mut ws_purl_by_importer,
        queue: &mut importer_queue,
    };
    while let Some(importer_id) = walk.queue.pop() {
        if !visited_importers.insert(importer_id.clone()) {
            continue;
        }
        let Some(importer) = lockfile.importers.get(importer_id.as_str()) else {
            continue;
        };
        collect_importer_components(
            &ImporterComponents {
                lockfile,
                lockfile_dir: &lockfile_dir,
                include,
                exclude_peers,
                root_purl: &root_purl,
                ctx: &ctx,
            },
            &importer_id,
            importer,
            &mut walk,
        );
    }

    Ok(SbomResult {
        root_name,
        root_version,
        root_type: sbom_type,
        root_license,
        root_description,
        root_author,
        root_repository,
        root_bugs_url,
        components: components_map.into_values().collect(),
        relationships,
    })
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
struct ImporterComponents<'a> {
    lockfile: &'a pnpm_lockfile::Lockfile,
    lockfile_dir: &'a Path,
    include: &'a IncludeFilter,
    exclude_peers: bool,
    root_purl: &'a str,
    ctx: &'a WalkContext<'a>,
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

    let importer_peer_names = if inputs.exclude_peers {
        confined_importer_dir(inputs.lockfile_dir, importer_id)
            .and_then(|dir| safe_read_package_json_from_dir(&dir).ok().flatten())
            .map(|manifest| peer_names_from_manifest(&manifest))
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
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

    let dep_maps = [
        inputs.include.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
        inputs.include.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
        inputs
            .include
            .optional_dependencies
            .then_some(importer.optional_dependencies.as_ref())
            .flatten(),
    ];
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

struct PkgMetadata {
    license: Option<String>,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    bugs_url: Option<String>,
}

fn read_pkg_metadata_from_store(
    key: &PkgNameVerPeer,
    pkg_name: &str,
    ctx: &WalkContext<'_>,
) -> PkgMetadata {
    let empty = PkgMetadata {
        license: None,
        description: None,
        author: None,
        homepage: None,
        repository: None,
        bugs_url: None,
    };
    let store_name = key.to_virtual_store_name(ctx.virtual_store_dir_max_length);
    for virtual_store_dir in ctx.virtual_store_dirs {
        let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
        if let Ok(Some(manifest)) = safe_read_package_json_from_dir(&pkg_dir) {
            return PkgMetadata {
                license: manifest.get("license").and_then(|v| v.as_str()).map(ToString::to_string),
                description: manifest
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                author: extract_author(&manifest),
                homepage: extract_homepage(&manifest),
                repository: extract_repository(&manifest),
                bugs_url: extract_bugs_url(&manifest),
            };
        }
    }
    empty
}

/// Whether `package` is an optional dependency that pnpm would not install on
/// this host, matching the `pnpm licenses` filter. Such a package is recorded
/// in the lockfile but never fetched, so no metadata can be read for it from
/// the virtual store.
fn platform_incompatible_optional(
    name: &str,
    snapshot_optional: bool,
    package: Option<&PackageMetadata>,
    installability: &InstallabilityOptions<'_>,
) -> bool {
    if !snapshot_optional {
        return false;
    }
    let Some(package) = package else {
        return false;
    };
    !platform_is_supported_with_inference(
        name,
        WantedPlatformRef {
            os: package.os.as_deref(),
            cpu: package.cpu.as_deref(),
            libc: package.libc.as_deref(),
        },
        installability,
    )
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

/// Whether the run asked for a subset of the workspace: any `--filter` /
/// `--filter-prod` selector, or `--workspace-root`. Without one, every
/// importer in the lockfile is in scope.
fn selectors_narrow_the_run(config: &Config) -> bool {
    !config.filter.is_empty() || !config.filter_prod.is_empty() || config.workspace_root
}

/// The lockfile importer ids of the workspace projects the run's selectors
/// selected.
fn selected_workspace_importer_ids(state: &State) -> miette::Result<HashSet<String>> {
    let project_dir = state.project_dir();
    let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(project_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
    let selection =
        select_recursive_projects(&projects, state.config, project_dir, AutoExcludeRoot::Disabled)?;
    Ok(selected_importer_ids(&selection, state.lockfile_dir()).into_iter().collect())
}

/// The selected importer ids the lockfile has no entry for, sorted so the
/// error names them in a stable order.
fn missing_importers(selected: &HashSet<String>, lockfile_ids: &[String]) -> Vec<String> {
    let known: HashSet<&str> = lockfile_ids.iter().map(String::as_str).collect();
    let mut missing: Vec<String> =
        selected.iter().filter(|id| !known.contains(id.as_str())).cloned().collect();
    missing.sort_unstable();
    missing
}

fn missing_importers_error(missing: &[String], project_kind: &str) -> miette::Report {
    let plural = if missing.len() == 1 { "" } else { "s" };
    let names = missing.join(", ");
    let lockfile_name = pnpm_lockfile::Lockfile::FILE_NAME;
    miette::miette!(
        code = "ERR_PNPM_SBOM_MISSING_IMPORTERS",
        r#"{lockfile_name} has no entry for the {project_kind} workspace project{plural}: {names}. Run "pnpm install" to update it."#,
    )
}

fn extend_dedicated_lockfile_map<Key, Value>(
    current: &mut HashMap<Key, Value>,
    incoming: HashMap<Key, Value>,
    entry_kind: &str,
    selected_dir: &Path,
) -> miette::Result<()>
where
    Key: Display + Eq + Hash,
    Value: PartialEq,
{
    for (key, value) in incoming {
        match current.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(entry) if entry.get() != &value => {
                let key = entry.key();
                let selected_dir = selected_dir.display();
                return Err(miette::miette!(
                    code = "ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES",
                    "Cannot combine dedicated workspace lockfiles because {} contains a different {entry_kind} entry for {key}",
                    selected_dir,
                ));
            }
            Entry::Occupied(_) => {}
        }
    }
    Ok(())
}

fn extend_dedicated_snapshots(
    current: &mut HashMap<PackageKey, SnapshotEntry>,
    incoming: HashMap<PackageKey, SnapshotEntry>,
    selected_dir: &Path,
) -> miette::Result<()> {
    for (key, mut value) in incoming {
        match current.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                let incoming_optional = value.optional;
                value.optional = entry.get().optional;
                if entry.get() != &value {
                    let key = entry.key();
                    let selected_dir = selected_dir.display();
                    return Err(miette::miette!(
                        code = "ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES",
                        "Cannot combine dedicated workspace lockfiles because {} contains a different snapshot entry for {key}",
                        selected_dir,
                    ));
                }
                entry.get_mut().optional &= incoming_optional;
            }
        }
    }
    Ok(())
}

fn extend_dedicated_lockfile(
    current: &mut Lockfile,
    incoming: Lockfile,
    selected_dir: &Path,
) -> miette::Result<()> {
    extend_dedicated_lockfile_map(
        &mut current.importers,
        incoming.importers,
        "importer",
        selected_dir,
    )?;
    if let Some(packages) = incoming.packages {
        extend_dedicated_lockfile_map(
            current.packages.get_or_insert_default(),
            packages,
            "package",
            selected_dir,
        )?;
    }
    if let Some(snapshots) = incoming.snapshots {
        extend_dedicated_snapshots(
            current.snapshots.get_or_insert_default(),
            snapshots,
            selected_dir,
        )?;
    }
    Ok(())
}

fn selected_and_reachable_project_dirs(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
) -> Vec<PathBuf> {
    let graph = selection.full_graph();
    let mut project_dirs: Vec<PathBuf> = selection.selected.keys().cloned().collect();
    let mut seen: HashSet<PathBuf> = project_dirs.iter().cloned().collect();
    let mut index = 0;
    while let Some(project_dir) = project_dirs.get(index) {
        if let Some(project) = graph.get(project_dir) {
            for dependency_dir in &project.dependencies {
                if seen.insert(dependency_dir.clone()) {
                    project_dirs.push(dependency_dir.clone());
                }
            }
        }
        index += 1;
    }
    project_dirs
}

fn merged_dedicated_lockfile_state(mut state: State) -> miette::Result<(State, Vec<PathBuf>)> {
    let project_dir = state.project_dir();
    let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(project_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
    let selection =
        select_recursive_projects(&projects, state.config, project_dir, AutoExcludeRoot::Disabled)?;

    let mut merged: Option<Lockfile> = None;
    let project_dirs = selected_and_reachable_project_dirs(&selection);
    let required_importer_ids: HashSet<String> = project_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect();
    let mut virtual_store_dirs = Vec::with_capacity(project_dirs.len());
    for selected_dir in &project_dirs {
        let mut project_config = state.config.clone();
        project_config.anchor_lockfile_paths(selected_dir);
        virtual_store_dirs.push(project_config.effective_virtual_store_dir().to_path_buf());

        let Some(mut lockfile) =
            Lockfile::load_wanted(selected_dir, &state.config.wanted_lockfile_selection())
                .map_err(miette::Report::new)?
        else {
            continue;
        };
        let importers = std::mem::take(&mut lockfile.importers);
        lockfile.importers = importers
            .into_iter()
            .map(|(importer_id, importer)| {
                validate_importer_id(&importer_id).map_err(miette::Report::new)?;
                let importer_dir = importer_root_dir(selected_dir, &importer_id);
                let workspace_id =
                    pnpm_workspace::importer_id_from_root_dir(workspace_root, &importer_dir);
                Ok((workspace_id, importer))
            })
            .collect::<miette::Result<_>>()?;
        if let Some(current) = &mut merged {
            extend_dedicated_lockfile(current, lockfile, selected_dir)?;
        } else {
            merged = Some(lockfile);
        }
    }

    if let Some(lockfile) = &merged {
        let importer_ids: Vec<String> = lockfile.importers.keys().cloned().collect();
        let missing = missing_importers(&required_importer_ids, &importer_ids);
        if !missing.is_empty() {
            return Err(missing_importers_error(&missing, "selected or reachable"));
        }
    }

    virtual_store_dirs.sort_unstable();
    virtual_store_dirs.dedup();
    let mut config = state.config.clone();
    config.lockfile_dir = Some(workspace_root.to_path_buf());
    state.config = Config::leak(config);
    state.lockfile = LazyLockfile::preloaded(merged);
    Ok((state, virtual_store_dirs))
}

impl SbomArgs {
    /// A workspace whose projects keep their own lockfiles has no one
    /// lockfile to walk, so a run that spans several of them merges
    /// their lockfiles — and their virtual stores — first.
    fn merged_state(&self, state: State) -> miette::Result<(State, Option<Vec<PathBuf>>)> {
        let spans_several_projects =
            state.config.recursive || self.split || selectors_narrow_the_run(state.config);
        if state.config.shares_one_lockfile() || !spans_several_projects {
            return Ok((state, None));
        }
        let (state, virtual_store_dirs) = merged_dedicated_lockfile_state(state)?;
        Ok((state, Some(virtual_store_dirs)))
    }

    pub async fn run(self, state: State) -> miette::Result<()> {
        let (state, virtual_store_dirs) = self.merged_state(state)?;
        self.check_spec_version()?;

        let include = self.include_filter(state.config.optional);
        let authors: Vec<String> = self
            .authors
            .as_deref()
            .map(|csv| {
                csv.split(',')
                    .map(|author| author.trim().to_string())
                    .filter(|author| !author.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        // `importers` is a `HashMap`, so its iteration order is arbitrary.
        // Sorting fixes the order `--split` emits its SBOMs in, and matches
        // the lockfile, whose importers are serialized sorted by id.
        let mut all_importer_ids: Vec<String> =
            lockfile.as_ref().map(|lf| lf.importers.keys().cloned().collect()).unwrap_or_default();
        all_importer_ids.sort_unstable();

        let all_count = all_importer_ids.len();
        let Some(importer_ids) = select_importer_ids(&state, all_importer_ids, lockfile.is_some())?
        else {
            return Ok(());
        };

        let should_split = self.split
            || (self.out.as_ref().is_some_and(|o| o.contains("%s")) && importer_ids.len() > 1);

        if should_split {
            return self.write_split_sboms(
                &state,
                &include,
                &authors,
                &importer_ids,
                virtual_store_dirs.as_deref(),
            );
        }
        let filter_ids: Option<Vec<&str>> = (selectors_narrow_the_run(state.config)
            || importer_ids.len() < all_count)
            .then(|| importer_ids.iter().map(String::as_str).collect());
        let result = collect_components(
            &state,
            &include,
            self.sbom_type,
            self.exclude_peers,
            self.lockfile_only,
            filter_ids.as_deref(),
            virtual_store_dirs.as_deref(),
        )?;
        let output = self.serialize(&result, &authors, false);
        let mut stdout = std::io::stdout();
        if let Some(out_template) = self.out.as_deref() {
            let file_path = write_sbom_file(out_template, &result, &output)?;
            let _ = writeln!(stdout, "{file_path}");
        } else {
            let _ = write!(stdout, "{output}");
        }
        let _ = stdout.flush();
        Ok(())
    }

    /// `--sbom-spec-version` names a `CycloneDX` version, so it applies to
    /// that format alone.
    fn check_spec_version(&self) -> miette::Result<()> {
        let Some(spec_ver) = self.spec_version.as_deref() else {
            return Ok(());
        };
        if self.format != SbomFormat::CycloneDx {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_SPEC_VERSION_UNSUPPORTED_FORMAT",
                "The --sbom-spec-version option is only supported with --sbom-format cyclonedx."
            ));
        }
        if !["1.5", "1.6", "1.7"].contains(&spec_ver) {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_INVALID_SPEC_VERSION",
                r#"Invalid CycloneDX spec version "{spec_ver}". Supported versions: 1.5, 1.6, 1.7."#
            ));
        }
        Ok(())
    }

    fn serialize(&self, result: &SbomResult, authors: &[String], compact: bool) -> String {
        match self.format {
            SbomFormat::CycloneDx => serialize_cyclonedx(&CycloneDxOpts {
                result,
                spec_version: self.spec_version.as_deref(),
                lockfile_only: self.lockfile_only,
                authors,
                supplier: self.supplier.as_deref(),
                compact,
            }),
            SbomFormat::Spdx => serialize_spdx(result, compact),
        }
    }

    /// One SBOM per selected project: written to the `%s`-templated
    /// paths, or streamed as NDJSON when there is no `--out`.
    fn write_split_sboms(
        &self,
        state: &State,
        include: &IncludeFilter,
        authors: &[String],
        importer_ids: &[String],
        virtual_store_dirs: Option<&[PathBuf]>,
    ) -> miette::Result<()> {
        if let Some(out) = self.out.as_deref()
            && !out.contains("%s")
        {
            return Err(miette::miette!(
                code = "ERR_PNPM_SBOM_OUT_MISSING_PLACEHOLDER",
                "When using --split with --out, the path must contain %s as a placeholder for the package name."
            ));
        }

        let compact = self.out.is_none();
        let mut ndjson_lines: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        let mut written_paths: HashSet<String> = HashSet::new();
        for importer_id in importer_ids {
            let filter = [importer_id.as_str()];
            let result = collect_components(
                state,
                include,
                self.sbom_type,
                self.exclude_peers,
                self.lockfile_only,
                Some(&filter),
                virtual_store_dirs,
            )?;
            // A project with no manifest of its own describes nothing.
            if result.root_name == "unknown" {
                continue;
            }
            let output = self.serialize(&result, authors, compact);
            let Some(out_template) = self.out.as_deref() else {
                ndjson_lines.push(output);
                continue;
            };
            let file_path = write_sbom_file(out_template, &result, &output)?;
            if !written_paths.insert(file_path.clone()) {
                return Err(miette::miette!(
                    code = "ERR_PNPM_SBOM_OUT_PATH_COLLISION",
                    r#"Multiple workspace packages resolve to the same output path "{file_path}". Include %v in the --out pattern to disambiguate."#
                ));
            }
            files.push(file_path);
        }

        let mut stdout = std::io::stdout();
        if self.out.is_some() {
            let _ = writeln!(
                stdout,
                "Generated {} SBOMs:\n{}",
                files.len(),
                files.iter().map(|file| format!("  {file}")).collect::<Vec<_>>().join("\n"),
            );
        } else {
            let _ = write!(stdout, "{}", ndjson_lines.join("\n"));
        }
        let _ = stdout.flush();
        Ok(())
    }
}

/// The importers the SBOM covers, in lockfile order. `None` when the
/// run's selectors matched no project: pnpm skips such a command, so an
/// SBOM of no project is never written.
fn select_importer_ids(
    state: &State,
    all_importer_ids: Vec<String>,
    has_lockfile: bool,
) -> miette::Result<Option<Vec<String>>> {
    if !selectors_narrow_the_run(state.config) {
        return Ok(Some(all_importer_ids));
    }
    let selected = selected_workspace_importer_ids(state)?;
    if selected.is_empty() {
        let workspace_dir = notice_workspace_dir(state.config, state.project_dir());
        println!("{}", no_projects_matched_message(workspace_dir));
        return Ok(None);
    }
    // Selecting through the workspace can name a project the lockfile has
    // no importer for, which only an out-of-date lockfile produces — pnpm
    // writes an entry for every project, `{}` for one with no
    // dependencies. Walking what is left would answer with an SBOM that
    // under-reports the selection's dependencies, so the run fails
    // instead. No lockfile at all is a different failure, left to
    // `collect_components` so it keeps its own error.
    let missing =
        if has_lockfile { missing_importers(&selected, &all_importer_ids) } else { Vec::new() };
    if !missing.is_empty() {
        return Err(missing_importers_error(&missing, "selected"));
    }
    // Intersecting rather than mapping the selection keeps the lockfile
    // order established by the caller.
    Ok(Some(all_importer_ids.into_iter().filter(|id| selected.contains(id)).collect()))
}

/// Write one SBOM to its `%s` / `%v`-templated path, returning it.
fn write_sbom_file(
    out_template: &str,
    result: &SbomResult,
    output: &str,
) -> miette::Result<String> {
    let sanitized_name = sanitize_path_segment(&sanitize_package_name(&result.root_name));
    let sanitized_ver = sanitize_path_segment(&result.root_version);
    let file_path = out_template.replace("%s", &sanitized_name).replace("%v", &sanitized_ver);
    let path = std::path::Path::new(&file_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| miette::miette!("create directory for {file_path}: {err}"))?;
    }
    std::fs::write(path, output)
        .map_err(|err| miette::miette!("write SBOM to {file_path}: {err}"))?;
    Ok(file_path)
}

struct CycloneDxOpts<'a> {
    result: &'a SbomResult,
    spec_version: Option<&'a str>,
    lockfile_only: bool,
    authors: &'a [String],
    supplier: Option<&'a str>,
    compact: bool,
}

fn split_scoped_name(name: &str) -> (Option<&str>, &str) {
    if name.starts_with('@') {
        if let Some(idx) = name.find('/') {
            (Some(&name[..idx]), &name[idx + 1..])
        } else {
            (None, name)
        }
    } else {
        (None, name)
    }
}

fn serialize_cyclonedx(opts: &CycloneDxOpts<'_>) -> String {
    let result = opts.result;
    let spec_version = opts.spec_version.unwrap_or("1.7");
    let root_type = match result.root_type {
        SbomComponentType::Library => "library",
        SbomComponentType::Application => "application",
    };

    let root_purl = build_purl(&result.root_name, &result.root_version);
    let root_component = cyclonedx_root_component(result, &root_purl, root_type);
    let components: Vec<serde_json::Value> =
        result.components.iter().map(cyclonedx_component).collect();
    let dependencies = cyclonedx_dependencies(result, &root_purl);

    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let phase = if opts.lockfile_only { "pre-build" } else { "build" };

    let mut metadata = serde_json::json!({
        "timestamp": timestamp,
        "lifecycles": [{ "phase": phase }],
        "tools": { "components": [{
            "type": "application",
            "name": "pnpm",
            "version": pnpm_config::PNPM_VERSION,
        }] },
        "component": root_component,
    });

    if !opts.authors.is_empty() {
        let author_list: Vec<serde_json::Value> =
            opts.authors.iter().map(|name| serde_json::json!({ "name": name })).collect();
        metadata["authors"] = serde_json::Value::Array(author_list);
    }
    if let Some(supplier) = opts.supplier {
        metadata["supplier"] = serde_json::json!({ "name": supplier });
    }

    let bom = serde_json::json!({
        "$schema": format!("http://cyclonedx.org/schema/bom-{spec_version}.schema.json"),
        "bomFormat": "CycloneDX",
        "specVersion": spec_version,
        "serialNumber": format!("urn:uuid:{}", generate_uuid_v4()),
        "version": 1,
        "metadata": metadata,
        "components": components,
        "dependencies": dependencies,
    });

    if opts.compact {
        serde_json::to_string(&bom).expect("JSON serialization")
    } else {
        serde_json::to_string_pretty(&bom).expect("JSON serialization")
    }
}

/// The `metadata.component` describing the project the SBOM is for.
fn cyclonedx_root_component(
    result: &SbomResult,
    root_purl: &str,
    root_type: &str,
) -> serde_json::Value {
    let (root_group, root_name) = split_scoped_name(&result.root_name);
    let mut root_component = serde_json::json!({
        "type": root_type,
        "name": root_name,
        "version": result.root_version,
        "purl": root_purl,
        "bom-ref": root_purl,
    });
    if let Some(group) = root_group {
        root_component["group"] = serde_json::Value::String(group.to_string());
    }
    if let Some(description) = &result.root_description {
        root_component["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &result.root_author {
        root_component["authors"] = serde_json::json!([{ "name": author }]);
    }
    if let Some(license) = &result.root_license {
        root_component["licenses"] = serde_json::json!([classify_license(license)]);
    }
    let mut root_ext_refs: Vec<serde_json::Value> = Vec::new();
    if let Some(repository) = &result.root_repository {
        root_ext_refs.push(serde_json::json!({ "type": "vcs", "url": repository }));
    }
    if let Some(bugs) = &result.root_bugs_url {
        root_ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
    }
    if !root_ext_refs.is_empty() {
        root_component["externalReferences"] = serde_json::Value::Array(root_ext_refs);
    }
    root_component
}

/// One installed package as a `CycloneDX` component.
fn cyclonedx_component(component: &SbomComponent) -> serde_json::Value {
    let (group, name) = split_scoped_name(&component.name);
    let mut comp = serde_json::json!({
        "type": "library",
        "name": name,
        "version": component.version,
        "purl": component.purl,
        "bom-ref": component.purl,
    });
    if let Some(group) = group {
        comp["group"] = serde_json::Value::String(group.to_string());
    }
    if component.dep_type == DepType::DevOnly {
        comp["scope"] = serde_json::Value::String("excluded".to_string());
        comp["properties"] =
            serde_json::json!([{ "name": "cdx:npm:package:development", "value": "true" }]);
    }
    if let Some(description) = &component.description {
        comp["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &component.author {
        comp["authors"] = serde_json::json!([{ "name": author }]);
    }
    if let Some(license) = &component.license {
        comp["licenses"] = serde_json::json!([classify_license(license)]);
    }
    let ext_refs = cyclonedx_external_references(component);
    if !ext_refs.is_empty() {
        comp["externalReferences"] = serde_json::Value::Array(ext_refs);
    }
    comp
}

/// Where a component came from: its archive (with the integrity the
/// lockfile pins), its homepage, its repository and its issue tracker.
fn cyclonedx_external_references(component: &SbomComponent) -> Vec<serde_json::Value> {
    let mut ext_refs: Vec<serde_json::Value> = Vec::new();
    if let Some(tarball) = &component.tarball_url {
        let mut dist_ref = serde_json::json!({ "type": "distribution", "url": tarball });
        if let Some(integrity) = &component.integrity
            && let Some(hashes) = integrity_to_hashes(integrity)
        {
            dist_ref["hashes"] = serde_json::Value::Array(hashes);
        }
        ext_refs.push(dist_ref);
    }
    if let Some(homepage) = &component.homepage {
        ext_refs.push(serde_json::json!({ "type": "website", "url": homepage }));
    }
    if let Some(repository) = &component.repository {
        ext_refs.push(serde_json::json!({ "type": "vcs", "url": repository }));
    }
    if let Some(bugs) = &component.bugs_url {
        ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
    }
    ext_refs
}

/// The `dependencies` graph, with one entry per component — including
/// the ones nothing depends on — in a deterministic order.
fn cyclonedx_dependencies(result: &SbomResult, root_purl: &str) -> Vec<serde_json::Value> {
    let mut deps_map: HashMap<&str, Vec<&str>> = HashMap::new();
    deps_map.entry(root_purl).or_default();
    for component in &result.components {
        deps_map.entry(&component.purl).or_default();
    }
    for relationship in &result.relationships {
        deps_map.entry(&relationship.from).or_default().push(&relationship.to);
    }
    let mut refs: Vec<&&str> = deps_map.keys().collect();
    refs.sort_unstable();
    refs.iter()
        .map(|ref_purl| {
            let mut dep_list = deps_map[*ref_purl].clone();
            dep_list.sort_unstable();
            dep_list.dedup();
            serde_json::json!({ "ref": ref_purl, "dependsOn": dep_list })
        })
        .collect()
}

fn sanitize_spdx_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' { ch } else { '-' })
        .collect()
}

/// The SPDX package describing the project the SBOM is for.
fn spdx_root_package(
    result: &SbomResult,
    root_purl: &str,
    root_spdx_id: &str,
    root_purpose: &str,
) -> serde_json::Value {
    let license_value = result.root_license.as_deref().unwrap_or("NOASSERTION");
    let mut root_package = serde_json::json!({
        "SPDXID": root_spdx_id,
        "name": result.root_name,
        "versionInfo": result.root_version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": false,
        "primaryPackagePurpose": root_purpose,
        "licenseConcluded": license_value,
        "licenseDeclared": license_value,
        "copyrightText": "NOASSERTION",
        "externalRefs": [{
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": root_purl,
        }],
    });
    if let Some(description) = &result.root_description {
        root_package["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &result.root_author {
        root_package["supplier"] = serde_json::Value::String(format!("Person: {author}"));
    }
    if let Some(repository) = &result.root_repository {
        root_package["homepage"] = serde_json::Value::String(repository.clone());
    }
    root_package
}

/// One installed package as an SPDX package.
fn spdx_component_package(component: &SbomComponent, spdx_id: &str) -> serde_json::Value {
    let comp_license = component.license.as_deref().unwrap_or("NOASSERTION");
    let download_loc = component.tarball_url.as_deref().unwrap_or("NOASSERTION");
    let mut pkg = serde_json::json!({
        "SPDXID": spdx_id,
        "name": component.name,
        "versionInfo": component.version,
        "downloadLocation": download_loc,
        "filesAnalyzed": false,
        "licenseConcluded": comp_license,
        "licenseDeclared": comp_license,
        "copyrightText": "NOASSERTION",
        "externalRefs": [{
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": component.purl,
        }],
    });
    if let Some(description) = &component.description {
        pkg["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(homepage) = &component.homepage {
        pkg["homepage"] = serde_json::Value::String(homepage.clone());
    }
    if let Some(author) = &component.author {
        pkg["supplier"] = serde_json::Value::String(format!("Person: {author}"));
    }
    if let Some(integrity) = &component.integrity
        && let Some(checksums) = integrity_to_spdx_checksums(integrity)
    {
        pkg["checksums"] = serde_json::Value::Array(checksums);
    }
    pkg
}

/// The document's relationships: it describes the root package, and each
/// dependency edge between packages it lists, deduplicated.
fn spdx_relationships(
    result: &SbomResult,
    root_spdx_id: &str,
    spdx_id_map: &HashMap<&str, String>,
) -> Vec<serde_json::Value> {
    let mut relationships: Vec<serde_json::Value> = vec![serde_json::json!({
        "spdxElementId": "SPDXRef-DOCUMENT",
        "relatedSpdxElement": root_spdx_id,
        "relationshipType": "DESCRIBES",
    })];
    let mut seen_rels: HashSet<(&str, &str)> = HashSet::new();
    for relationship in &result.relationships {
        let (Some(from_id), Some(to_id)) = (
            spdx_id_map.get(relationship.from.as_str()),
            spdx_id_map.get(relationship.to.as_str()),
        ) else {
            continue;
        };
        if !seen_rels.insert((from_id.as_str(), to_id.as_str())) {
            continue;
        }
        relationships.push(serde_json::json!({
            "spdxElementId": from_id,
            "relatedSpdxElement": to_id,
            "relationshipType": "DEPENDS_ON",
        }));
    }
    relationships
}

fn serialize_spdx(result: &SbomResult, compact: bool) -> String {
    let root_purl = build_purl(&result.root_name, &result.root_version);
    let root_spdx_id = "SPDXRef-RootPackage";
    let root_purpose = match result.root_type {
        SbomComponentType::Library => "LIBRARY",
        SbomComponentType::Application => "APPLICATION",
    };

    let mut spdx_id_map: HashMap<&str, String> = HashMap::new();
    spdx_id_map.insert(&root_purl, root_spdx_id.to_string());
    let mut spdx_packages = vec![spdx_root_package(result, &root_purl, root_spdx_id, root_purpose)];
    for (index, component) in result.components.iter().enumerate() {
        let spdx_id = format!(
            "SPDXRef-Package-{}-{}-{index}",
            sanitize_spdx_id(&component.name),
            sanitize_spdx_id(&component.version),
        );
        spdx_id_map.insert(&component.purl, spdx_id.clone());
        spdx_packages.push(spdx_component_package(component, &spdx_id));
    }
    let spdx_relationships = spdx_relationships(result, root_spdx_id, &spdx_id_map);

    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let doc_namespace = format!(
        "https://spdx.org/spdxdocs/{}-{}-{}",
        sanitize_spdx_id(&result.root_name),
        result.root_version,
        generate_uuid_v4(),
    );

    let doc = serde_json::json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": result.root_name,
        "documentNamespace": doc_namespace,
        "creationInfo": {
            "created": timestamp,
            "creators": ["Tool: pnpm"],
        },
        "packages": spdx_packages,
        "relationships": spdx_relationships,
    });

    if compact {
        serde_json::to_string(&doc).expect("JSON serialization")
    } else {
        serde_json::to_string_pretty(&doc).expect("JSON serialization")
    }
}

fn integrity_to_hashes(integrity: &str) -> Option<Vec<serde_json::Value>> {
    let mut hashes = Vec::new();
    for part in integrity.split_whitespace() {
        let Some((alg, hash)) = part.split_once('-') else { continue };
        let cdx_alg = match alg {
            "sha1" => "SHA-1",
            "sha256" => "SHA-256",
            "sha384" => "SHA-384",
            "sha512" => "SHA-512",
            "md5" => "MD5",
            _ => continue,
        };
        let hex = base64_to_hex(hash)?;
        hashes.push(serde_json::json!({
            "alg": cdx_alg,
            "content": hex,
        }));
    }
    if hashes.is_empty() { None } else { Some(hashes) }
}

fn integrity_to_spdx_checksums(integrity: &str) -> Option<Vec<serde_json::Value>> {
    let mut checksums = Vec::new();
    for part in integrity.split_whitespace() {
        let Some((alg, hash)) = part.split_once('-') else { continue };
        let spdx_alg = match alg {
            "sha1" => "SHA1",
            "sha256" => "SHA256",
            "sha384" => "SHA384",
            "sha512" => "SHA512",
            "md5" => "MD5",
            _ => continue,
        };
        let hex = base64_to_hex(hash)?;
        checksums.push(serde_json::json!({
            "algorithm": spdx_alg,
            "checksumValue": hex,
        }));
    }
    if checksums.is_empty() { None } else { Some(checksums) }
}

fn generate_uuid_v4() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    let half_a = hasher.finish();
    let mut hasher2 = state.build_hasher();
    hasher2.write_u64(!half_a);
    let half_b = hasher2.finish();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&half_a.to_le_bytes());
    bytes[8..].copy_from_slice(&half_b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    use std::fmt::Write;
    let mut uuid = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            uuid.push('-');
        }
        let _ = write!(uuid, "{byte:02x}");
    }
    uuid
}

fn normalize_link_path(base_importer_id: &str, link_target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base_importer_id == "." {
        Vec::new()
    } else {
        base_importer_id.split('/').filter(|segment| !segment.is_empty()).collect()
    };
    for segment in link_target.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() { Some(".".to_string()) } else { Some(parts.join("/")) }
}

fn sanitize_package_name(name: &str) -> String {
    name.strip_prefix('@').unwrap_or(name).replace('/', "-")
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                || ch.is_ascii_control()
            {
                '-'
            } else {
                ch
            }
        })
        .collect();
    if sanitized == "." || sanitized == ".." || sanitized.trim().is_empty() {
        "-".to_string()
    } else {
        sanitized
    }
}

fn base64_to_hex(input: &str) -> Option<String> {
    use base64::Engine;
    use std::fmt::Write;
    let bytes = base64::engine::general_purpose::STANDARD.decode(input).ok()?;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in &bytes {
        let _ = write!(hex, "{b:02x}");
    }
    Some(hex)
}

#[cfg(test)]
mod tests;
