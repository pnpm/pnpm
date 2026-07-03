//! `pacquet sbom` — generate a Software Bill of Materials.
//!
//! Ports pnpm's
//! [`sbom` command](https://github.com/pnpm/pnpm/blob/2b4952e804/pnpm11/deps/compliance/commands/src/sbom/sbom.ts).

use crate::State;
use clap::Args;
use indexmap::IndexMap;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotEntry,
};
use pacquet_package_manifest::safe_read_package_json_from_dir;
use std::{
    collections::{HashMap, HashSet},
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
    #[clap(long, short = 'P')]
    pub prod: bool,

    /// Only include dev dependencies.
    #[clap(long, short = 'D')]
    pub dev: bool,

    /// Exclude optional dependencies.
    #[clap(long = "no-optional")]
    pub no_optional: bool,

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
    fn include_filter(&self) -> IncludeFilter {
        IncludeFilter {
            dependencies: !self.dev,
            dev_dependencies: !self.prod,
            optional_dependencies: !self.prod && !self.no_optional,
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
    virtual_store_dir: Option<PathBuf>,
    virtual_store_dir_max_length: usize,
    include_optional_transitive: bool,
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

fn extract_author(manifest: &serde_json::Value) -> Option<String> {
    let author = manifest.get("author")?;
    if let Some(s) = author.as_str() {
        return Some(s.to_string());
    }
    author.get("name").and_then(|n| n.as_str()).map(ToString::to_string)
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

fn extract_homepage(manifest: &serde_json::Value) -> Option<String> {
    manifest.get("homepage").and_then(|v| v.as_str()).map(ToString::to_string)
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

fn integrity_string(resolution: &LockfileResolution) -> Option<String> {
    match resolution {
        LockfileResolution::Registry(r) => Some(r.integrity.to_string()),
        LockfileResolution::Tarball(r) => r.integrity.as_ref().map(ToString::to_string),
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
    lockfile: &pacquet_lockfile::Lockfile,
    include_optional_transitive: bool,
) -> HashMap<PackageKey, DepType> {
    let snapshots = lockfile.snapshots.as_ref();
    let mut dep_types: HashMap<PackageKey, DepType> = HashMap::new();
    let mut walked: HashSet<(PackageKey, bool)> = HashSet::new();

    let mut dev_keys: Vec<PackageKey> = Vec::new();
    let mut prod_keys: Vec<PackageKey> = Vec::new();

    for importer in lockfile.importers.values() {
        if let Some(deps) = &importer.dev_dependencies {
            for (name, spec) in deps {
                if let Some(key) = spec.version.resolved_key(name) {
                    dev_keys.push(key);
                }
            }
        }
        for deps in [&importer.dependencies, &importer.optional_dependencies].into_iter().flatten()
        {
            for (name, spec) in deps {
                if let Some(key) = spec.version.resolved_key(name) {
                    prod_keys.push(key);
                }
            }
        }
    }

    detect_dep_types_walk(
        snapshots,
        &mut dep_types,
        &mut walked,
        dev_keys,
        true,
        include_optional_transitive,
    );
    detect_dep_types_walk(
        snapshots,
        &mut dep_types,
        &mut walked,
        prod_keys,
        false,
        include_optional_transitive,
    );
    dep_types
}

fn detect_dep_types_walk(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    dep_types: &mut HashMap<PackageKey, DepType>,
    walked: &mut HashSet<(PackageKey, bool)>,
    initial_keys: Vec<PackageKey>,
    is_dev: bool,
    include_optional_transitive: bool,
) {
    let mut queue: Vec<PackageKey> = initial_keys;

    while let Some(key) = queue.pop() {
        let walk_key = (key.clone(), is_dev);
        if walked.contains(&walk_key) {
            continue;
        }
        walked.insert(walk_key);

        if is_dev {
            dep_types.entry(key.clone()).or_insert(DepType::DevOnly);
        } else if dep_types.get(&key) == Some(&DepType::DevOnly) {
            dep_types.insert(key.clone(), DepType::ProdOnly);
        } else {
            dep_types.entry(key.clone()).or_insert(DepType::ProdOnly);
        }

        let Some(snapshot) = snapshots.and_then(|s| s.get(&key)) else {
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

    let project_root_dir =
        state.manifest.path().parent().unwrap_or_else(|| Path::new(".")).to_path_buf();

    let manifest_value = match filter_importer_ids {
        Some(&[single_id]) if single_id != "." => {
            let importer_dir = project_root_dir.join(single_id);
            safe_read_package_json_from_dir(&importer_dir)
                .ok()
                .flatten()
                .unwrap_or_else(|| state.manifest.value().clone())
        }
        _ => state.manifest.value().clone(),
    };
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

    let virtual_store_dir = (!lockfile_only).then(|| project_root_dir.join("node_modules/.pnpm"));

    let ctx = WalkContext {
        snapshots: lockfile.snapshots.as_ref(),
        packages: lockfile.packages.as_ref(),
        dep_types: &dep_types,
        default_registry: &state.config.registry,
        virtual_store_dir,
        virtual_store_dir_max_length: state.config.virtual_store_dir_max_length as usize,
        include_optional_transitive: include.optional_dependencies,
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

    while let Some(importer_id) = importer_queue.pop() {
        if !visited_importers.insert(importer_id.clone()) {
            continue;
        }
        let Some(importer) = lockfile.importers.get(importer_id.as_str()) else {
            continue;
        };

        let parent_purl =
            ws_purl_by_importer.get(&importer_id).cloned().unwrap_or_else(|| root_purl.clone());

        let importer_peer_names = if exclude_peers {
            let importer_dir = if importer_id == "." {
                project_root_dir.clone()
            } else {
                project_root_dir.join(&importer_id)
            };
            safe_read_package_json_from_dir(&importer_dir)
                .ok()
                .flatten()
                .map(|m| peer_names_from_manifest(&m))
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

        let dep_maps: Vec<&HashMap<PkgName, _>> = [
            include.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
            include.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
            include
                .optional_dependencies
                .then_some(importer.optional_dependencies.as_ref())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();

        for deps in dep_maps {
            for (name, spec) in deps {
                if !importer_peer_names.is_empty()
                    && importer_peer_names.contains(&name.to_string())
                {
                    continue;
                }
                if let Some(link_target) = spec.version.as_link_target() {
                    if let Some(target_id) = normalize_link_path(&importer_id, link_target)
                        && lockfile.importers.contains_key(target_id.as_str())
                    {
                        let ws_dir = if target_id == "." {
                            project_root_dir.clone()
                        } else {
                            project_root_dir.join(&target_id)
                        };
                        if let Ok(Some(ws_manifest)) = safe_read_package_json_from_dir(&ws_dir) {
                            let ws_name = ws_manifest
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&name.to_string())
                                .to_string();
                            let ws_version = ws_manifest
                                .get("version")
                                .and_then(|v| v.as_str())
                                .unwrap_or("0.0.0")
                                .to_string();
                            let ws_purl = build_purl(&ws_name, &ws_version);
                            let name_str = name.to_string();
                            let dev_only = dev_dep_names.contains(&name_str)
                                && !prod_dep_names.contains(&name_str);
                            relationships.push(SbomRelationship {
                                from: parent_purl.clone(),
                                to: ws_purl.clone(),
                            });
                            let ws_dep_type =
                                if dev_only { DepType::DevOnly } else { DepType::ProdOnly };
                            if let Some(existing) = components_map.get_mut(&ws_purl) {
                                if !dev_only && existing.dep_type == DepType::DevOnly {
                                    existing.dep_type = DepType::ProdOnly;
                                }
                            } else {
                                components_map.insert(
                                    ws_purl.clone(),
                                    SbomComponent {
                                        name: ws_name,
                                        version: ws_version,
                                        purl: ws_purl.clone(),
                                        dep_type: ws_dep_type,
                                        integrity: None,
                                        tarball_url: None,
                                        license: ws_manifest
                                            .get("license")
                                            .and_then(|v| v.as_str())
                                            .map(ToString::to_string),
                                        description: ws_manifest
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .map(ToString::to_string),
                                        author: extract_author(&ws_manifest),
                                        homepage: extract_homepage(&ws_manifest),
                                        repository: extract_repository(&ws_manifest),
                                        bugs_url: extract_bugs_url(&ws_manifest),
                                    },
                                );
                            }
                            ws_purl_by_importer.insert(target_id.clone(), ws_purl);
                            importer_queue.push(target_id);
                        }
                    }
                } else if let Some(snapshot_key) = spec.version.resolved_key(name) {
                    walk_snapshot(
                        &snapshot_key,
                        parent_purl.clone(),
                        &ctx,
                        &mut components_map,
                        &mut relationships,
                        &mut visited,
                    );
                }
            }
        }
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
    let Some(ref vs_dir) = ctx.virtual_store_dir else {
        return empty;
    };
    let store_name = key.to_virtual_store_name(ctx.virtual_store_dir_max_length);
    let pkg_dir = vs_dir.join(&store_name).join("node_modules").join(pkg_name);
    let Ok(Some(manifest)) = safe_read_package_json_from_dir(&pkg_dir) else {
        return empty;
    };
    PkgMetadata {
        license: manifest.get("license").and_then(|v| v.as_str()).map(ToString::to_string),
        description: manifest.get("description").and_then(|v| v.as_str()).map(ToString::to_string),
        author: extract_author(&manifest),
        homepage: extract_homepage(&manifest),
        repository: extract_repository(&manifest),
        bugs_url: extract_bugs_url(&manifest),
    }
}

fn walk_snapshot(
    initial_key: &PkgNameVerPeer,
    initial_parent_purl: String,
    ctx: &WalkContext<'_>,
    components_map: &mut IndexMap<String, SbomComponent>,
    relationships: &mut Vec<SbomRelationship>,
    visited: &mut HashSet<PackageKey>,
) {
    let mut queue: Vec<(PkgNameVerPeer, String)> = vec![(initial_key.clone(), initial_parent_purl)];

    while let Some((key, parent_purl)) = queue.pop() {
        let name = key.name.to_string();
        let version = ctx
            .packages
            .and_then(|pkgs| pkgs.get(&key.without_peer()))
            .and_then(|meta| meta.version.clone())
            .unwrap_or_else(|| key.suffix.version().to_string());

        let purl = build_purl(&name, &version);

        relationships.push(SbomRelationship { from: parent_purl, to: purl.clone() });

        if !visited.insert(key.clone()) {
            continue;
        }

        if !components_map.contains_key(&purl) {
            let pkg_meta = ctx.packages.and_then(|pkgs| pkgs.get(&key.without_peer()));
            let integrity = pkg_meta.and_then(|meta| integrity_string(&meta.resolution));
            let tarball_url = pkg_meta.and_then(|meta| {
                tarball_url_for_component(&meta.resolution, &name, &version, ctx.default_registry)
            });

            let store_meta = read_pkg_metadata_from_store(&key, &name, ctx);
            let dep_type = ctx.dep_types.get(&key).copied().unwrap_or(DepType::ProdOnly);

            components_map.insert(
                purl.clone(),
                SbomComponent {
                    name,
                    version,
                    purl: purl.clone(),
                    dep_type,
                    integrity,
                    tarball_url,
                    license: store_meta.license,
                    description: store_meta.description,
                    author: store_meta.author,
                    homepage: store_meta.homepage,
                    repository: store_meta.repository,
                    bugs_url: store_meta.bugs_url,
                },
            );
        }

        let Some(snapshot) = ctx.snapshots.and_then(|s| s.get(&key)) else {
            continue;
        };

        let optional_iter = ctx
            .include_optional_transitive
            .then(|| snapshot.optional_dependencies.iter().flatten())
            .into_iter()
            .flatten();
        for (alias, dep_ref) in snapshot.dependencies.iter().flatten().chain(optional_iter) {
            if let Some(child_key) = dep_ref.resolve(alias) {
                queue.push((child_key, purl.clone()));
            }
        }
    }
}

impl SbomArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        if let Some(ref spec_ver) = self.spec_version {
            if self.format != SbomFormat::CycloneDx {
                return Err(miette::miette!(
                    code = "ERR_PNPM_SBOM_SPEC_VERSION_UNSUPPORTED_FORMAT",
                    "The --sbom-spec-version option is only supported with --sbom-format cyclonedx."
                ));
            }
            if !["1.5", "1.6", "1.7"].contains(&spec_ver.as_str()) {
                return Err(miette::miette!(
                    code = "ERR_PNPM_SBOM_INVALID_SPEC_VERSION",
                    r#"Invalid CycloneDX spec version "{spec_ver}". Supported versions: 1.5, 1.6, 1.7."#
                ));
            }
        }

        let include = self.include_filter();
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
        let all_importer_ids: Vec<String> =
            lockfile.as_ref().map(|lf| lf.importers.keys().cloned().collect()).unwrap_or_default();

        let all_count = all_importer_ids.len();
        let importer_ids = if state.config.filter.is_empty() {
            all_importer_ids
        } else {
            let project_root = state.manifest.path().parent().unwrap_or_else(|| Path::new("."));
            all_importer_ids
                .into_iter()
                .filter(|id| {
                    let importer_dir = if id == "." {
                        project_root.to_string_lossy().to_string()
                    } else {
                        project_root.join(id).to_string_lossy().to_string()
                    };
                    state.config.filter.iter().any(|f| {
                        let pattern = f.strip_prefix("./").unwrap_or(f);
                        id == pattern
                            || id.starts_with(&format!("{pattern}/"))
                            || importer_dir.ends_with(pattern)
                    })
                })
                .collect()
        };

        let should_split = self.split
            || (self.out.as_ref().is_some_and(|o| o.contains("%s")) && importer_ids.len() > 1);

        if should_split {
            if let Some(ref out) = self.out
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

            for importer_id in &importer_ids {
                let filter = [importer_id.as_str()];
                let result = collect_components(
                    &state,
                    &include,
                    self.sbom_type,
                    self.exclude_peers,
                    self.lockfile_only,
                    Some(&filter),
                )?;
                if result.root_name == "unknown" {
                    continue;
                }

                let output = match self.format {
                    SbomFormat::CycloneDx => serialize_cyclonedx(&CycloneDxOpts {
                        result: &result,
                        spec_version: self.spec_version.as_deref(),
                        lockfile_only: self.lockfile_only,
                        authors: &authors,
                        supplier: self.supplier.as_deref(),
                        compact,
                    }),
                    SbomFormat::Spdx => serialize_spdx(&result, compact),
                };

                if let Some(ref out_template) = self.out {
                    let sanitized_name =
                        sanitize_path_segment(&sanitize_package_name(&result.root_name));
                    let sanitized_ver = sanitize_path_segment(&result.root_version);
                    let file_path =
                        out_template.replace("%s", &sanitized_name).replace("%v", &sanitized_ver);
                    if written_paths.contains(&file_path) {
                        return Err(miette::miette!(
                            code = "ERR_PNPM_SBOM_OUT_PATH_COLLISION",
                            r#"Multiple workspace packages resolve to the same output path "{file_path}". Include %v in the --out pattern to disambiguate."#
                        ));
                    }
                    written_paths.insert(file_path.clone());
                    let path = std::path::Path::new(&file_path);

                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|err| {
                            miette::miette!("create directory for {file_path}: {err}")
                        })?;
                    }
                    std::fs::write(path, &output)
                        .map_err(|err| miette::miette!("write SBOM to {file_path}: {err}"))?;
                    files.push(file_path);
                } else {
                    ndjson_lines.push(output);
                }
            }

            let mut stdout = std::io::stdout();
            if self.out.is_some() {
                let _ = writeln!(
                    stdout,
                    "Generated {} SBOMs:\n{}",
                    files.len(),
                    files.iter().map(|f| format!("  {f}")).collect::<Vec<_>>().join("\n"),
                );
            } else {
                let _ = write!(stdout, "{}", ndjson_lines.join("\n"));
            }
            let _ = stdout.flush();
        } else {
            let filter_ids: Option<Vec<&str>> = (importer_ids.len() < all_count)
                .then(|| importer_ids.iter().map(String::as_str).collect());
            let result = collect_components(
                &state,
                &include,
                self.sbom_type,
                self.exclude_peers,
                self.lockfile_only,
                filter_ids.as_deref(),
            )?;

            let output = match self.format {
                SbomFormat::CycloneDx => serialize_cyclonedx(&CycloneDxOpts {
                    result: &result,
                    spec_version: self.spec_version.as_deref(),
                    lockfile_only: self.lockfile_only,
                    authors: &authors,
                    supplier: self.supplier.as_deref(),
                    compact: false,
                }),
                SbomFormat::Spdx => serialize_spdx(&result, false),
            };

            if let Some(ref out_template) = self.out {
                let sanitized_name =
                    sanitize_path_segment(&sanitize_package_name(&result.root_name));
                let sanitized_ver = sanitize_path_segment(&result.root_version);
                let file_path =
                    out_template.replace("%s", &sanitized_name).replace("%v", &sanitized_ver);
                let path = std::path::Path::new(&file_path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| {
                        miette::miette!("create directory for {file_path}: {err}")
                    })?;
                }
                std::fs::write(path, &output)
                    .map_err(|err| miette::miette!("write SBOM to {file_path}: {err}"))?;
                let mut stdout = std::io::stdout();
                let _ = writeln!(stdout, "{file_path}");
                let _ = stdout.flush();
            } else {
                let mut stdout = std::io::stdout();
                let _ = write!(stdout, "{output}");
                let _ = stdout.flush();
            }
        }

        Ok(())
    }
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
    if let Some(ref desc) = result.root_description {
        root_component["description"] = serde_json::Value::String(desc.clone());
    }
    if let Some(ref author) = result.root_author {
        root_component["authors"] = serde_json::json!([{ "name": author }]);
    }
    if let Some(ref license) = result.root_license {
        root_component["licenses"] = serde_json::json!([classify_license(license)]);
    }
    let mut root_ext_refs: Vec<serde_json::Value> = Vec::new();
    if let Some(ref repo) = result.root_repository {
        root_ext_refs.push(serde_json::json!({ "type": "vcs", "url": repo }));
    }
    if let Some(ref bugs) = result.root_bugs_url {
        root_ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
    }
    if !root_ext_refs.is_empty() {
        root_component["externalReferences"] = serde_json::Value::Array(root_ext_refs);
    }

    let components: Vec<serde_json::Value> = result
        .components
        .iter()
        .map(|component| {
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
                comp["properties"] = serde_json::json!([
                    { "name": "cdx:npm:package:development", "value": "true" }
                ]);
            }
            if let Some(ref desc) = component.description {
                comp["description"] = serde_json::Value::String(desc.clone());
            }
            if let Some(ref author) = component.author {
                comp["authors"] = serde_json::json!([{ "name": author }]);
            }
            if let Some(ref license) = component.license {
                comp["licenses"] = serde_json::json!([classify_license(license)]);
            }

            let mut ext_refs: Vec<serde_json::Value> = Vec::new();
            if let Some(ref tarball) = component.tarball_url {
                let mut dist_ref = serde_json::json!({ "type": "distribution", "url": tarball });
                if let Some(ref integrity) = component.integrity
                    && let Some(hashes) = integrity_to_hashes(integrity)
                {
                    dist_ref["hashes"] = serde_json::Value::Array(hashes);
                }
                ext_refs.push(dist_ref);
            }
            if let Some(ref hp) = component.homepage {
                ext_refs.push(serde_json::json!({ "type": "website", "url": hp }));
            }
            if let Some(ref repo) = component.repository {
                ext_refs.push(serde_json::json!({ "type": "vcs", "url": repo }));
            }
            if let Some(ref bugs) = component.bugs_url {
                ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
            }
            if !ext_refs.is_empty() {
                comp["externalReferences"] = serde_json::Value::Array(ext_refs);
            }
            comp
        })
        .collect();

    let dependencies: Vec<serde_json::Value> = {
        let mut deps_map: HashMap<&str, Vec<&str>> = HashMap::new();
        deps_map.entry(&root_purl).or_default();
        for c in &result.components {
            deps_map.entry(&c.purl).or_default();
        }
        for rel in &result.relationships {
            deps_map.entry(&rel.from).or_default().push(&rel.to);
        }
        let mut refs: Vec<&&str> = deps_map.keys().collect();
        refs.sort_unstable();
        refs.iter()
            .map(|ref_purl| {
                let mut dep_list = deps_map[*ref_purl].clone();
                dep_list.sort_unstable();
                dep_list.dedup();
                serde_json::json!({
                    "ref": ref_purl,
                    "dependsOn": dep_list,
                })
            })
            .collect()
    };

    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let phase = if opts.lockfile_only { "pre-build" } else { "build" };

    let mut metadata = serde_json::json!({
        "timestamp": timestamp,
        "lifecycles": [{ "phase": phase }],
        "tools": { "components": [{
            "type": "application",
            "name": "pacquet",
            "version": pacquet_config::PACQUET_VERSION,
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

fn sanitize_spdx_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' { ch } else { '-' })
        .collect()
}

fn serialize_spdx(result: &SbomResult, compact: bool) -> String {
    let root_purl = build_purl(&result.root_name, &result.root_version);
    let root_spdx_id = "SPDXRef-RootPackage";
    let root_purpose = match result.root_type {
        SbomComponentType::Library => "LIBRARY",
        SbomComponentType::Application => "APPLICATION",
    };

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
    if let Some(ref desc) = result.root_description {
        root_package["description"] = serde_json::Value::String(desc.clone());
    }
    if let Some(ref author) = result.root_author {
        root_package["supplier"] = serde_json::Value::String(format!("Person: {author}"));
    }
    if let Some(ref repo) = result.root_repository {
        root_package["homepage"] = serde_json::Value::String(repo.clone());
    }

    let mut spdx_id_map: HashMap<&str, String> = HashMap::new();
    spdx_id_map.insert(&root_purl, root_spdx_id.to_string());

    let mut spdx_packages = vec![root_package];

    for (i, component) in result.components.iter().enumerate() {
        let spdx_id = format!(
            "SPDXRef-Package-{}-{}-{i}",
            sanitize_spdx_id(&component.name),
            sanitize_spdx_id(&component.version),
        );
        spdx_id_map.insert(&component.purl, spdx_id.clone());

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

        if let Some(ref desc) = component.description {
            pkg["description"] = serde_json::Value::String(desc.clone());
        }
        if let Some(ref hp) = component.homepage {
            pkg["homepage"] = serde_json::Value::String(hp.clone());
        }
        if let Some(ref author) = component.author {
            pkg["supplier"] = serde_json::Value::String(format!("Person: {author}"));
        }

        if let Some(ref integrity) = component.integrity
            && let Some(checksums) = integrity_to_spdx_checksums(integrity)
        {
            pkg["checksums"] = serde_json::Value::Array(checksums);
        }

        spdx_packages.push(pkg);
    }

    let mut spdx_relationships: Vec<serde_json::Value> = vec![serde_json::json!({
        "spdxElementId": "SPDXRef-DOCUMENT",
        "relatedSpdxElement": root_spdx_id,
        "relationshipType": "DESCRIBES",
    })];

    let mut seen_rels: HashSet<(String, String)> = HashSet::new();
    for rel in &result.relationships {
        if let (Some(from_id), Some(to_id)) =
            (spdx_id_map.get(rel.from.as_str()), spdx_id_map.get(rel.to.as_str()))
        {
            let key = (from_id.clone(), to_id.clone());
            if seen_rels.insert(key) {
                spdx_relationships.push(serde_json::json!({
                    "spdxElementId": from_id,
                    "relatedSpdxElement": to_id,
                    "relationshipType": "DEPENDS_ON",
                }));
            }
        }
    }

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
            "creators": ["Tool: pacquet"],
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
