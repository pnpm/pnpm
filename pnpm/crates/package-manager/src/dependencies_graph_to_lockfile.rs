//! Adapter that converts the resolver's [`DependenciesGraph`] into a
//! [`Lockfile`].
//!
//! pacquet's [`Lockfile`] holds the v9 `packages:` and `snapshots:`
//! maps separately, so this adapter emits the two directly from the
//! graph rather than building one merged per-depPath snapshot and
//! fanning it out on write.

pub(crate) use importers::manifest_publish_config;
pub(crate) use packages::manifest_has_bin;

mod packages;

use packages::{PackageMetadataSources, build_packages_and_snapshots};

mod importers;

use importers::{build_importers, importer_resolved_version, manifest_alias_to_group};

use std::collections::{BTreeMap, HashMap, HashSet};

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{
    CatalogSnapshots, ComVer, Lockfile, LockfileFormError, LockfileSettings, LockfileVersion,
    PackageKey, PackageMetadata, ParseImporterDepVersionError, ParsePkgNameSuffixError,
    ParsePkgVerPeerError, ProjectSnapshot, RegistryOptions, ResolvedCatalogEntry,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{
    DepPath, DependenciesGraph, DependenciesGraphNode, UpdateReuseScope,
};
use serde_json::Value;

/// One importer's contribution to [`dependencies_graph_to_lockfile`].
///
/// Pacquet keeps the per-importer slice narrow — the manifest decides
/// the dep-group classification of each alias, and
/// `direct_dependencies_by_alias` (from `resolve_peers`) tells us which
/// `DepPath` each alias resolved to. The shared `DependenciesGraph`
/// lives outside this struct because it is importer-independent.
pub struct ImporterLockfileInput<'a> {
    /// The on-disk `package.json` for this importer. Used to source
    /// each direct dependency's specifier (the value the user wrote)
    /// and the importer-level dep-group classification
    /// (`dependencies` vs `devDependencies` vs `optionalDependencies`).
    pub manifest: &'a PackageManifest,
    /// `alias → DepPath` for the direct dependencies of this importer,
    /// as emitted by [`pnpm_resolving_deps_resolver::resolve_peers`].
    pub direct_dependencies_by_alias: BTreeMap<String, DepPath>,
}

/// The install-wide settings [`build_importer`](crate::dependencies_graph_to_lockfile::importers::build_importer) reads. Both decide
/// which direct dependencies reach the importer entry, so they travel
/// together instead of as two adjacent `bool` parameters.
struct ImporterLockfileFlags {
    exclude_links_from_lockfile: bool,
    auto_install_peers: bool,
}

/// Options threaded into [`dependencies_graph_to_lockfile`].
pub struct GraphToLockfileOptions<'a> {
    /// One entry per workspace project being installed. Keyed by the
    /// lockfile importer id (`"."` for the workspace root,
    /// `"packages/<name>"` for siblings — see
    /// [`pnpm_workspace::importer_id_from_root_dir`]).
    pub importers: BTreeMap<String, ImporterLockfileInput<'a>>,
    /// Cross-importer dedup graph keyed by `DepPath`. The fresh-resolve
    /// dispatch merges every per-importer `peers_result.graph` into
    /// this one map before calling — identical snapshot keys collapse
    /// onto one entry.
    pub graph: &'a DependenciesGraph,
    /// Round-tripped into the lockfile's top-level `settings:` block
    /// so a subsequent pnpm install can compare its own settings via
    /// `@pnpm/lockfile.settings-checker`'s `getOutdatedLockfileSetting`.
    pub auto_install_peers: bool,
    /// When `true`, the resolver ran with `dedupePeers` on.
    pub dedupe_peers: bool,
    pub exclude_links_from_lockfile: bool,
    /// `injectWorkspacePackages` recorded into the lockfile's
    /// `settings.injectWorkspacePackages`. `false` is omitted on save
    /// via [`LockfileSettings`]'s serde `skip_serializing_if`.
    pub inject_workspace_packages: bool,
    /// `peersSuffixMaxLength` round-tripped into the lockfile's
    /// `settings.peersSuffixMaxLength` so a later install detects
    /// drift via `@pnpm/lockfile.settings-checker`. Pass `None` when
    /// the value equals the default (1000) so the field is stripped
    /// from the serialized lockfile.
    pub peers_suffix_max_length: Option<u64>,
    /// `overrides` recorded into the lockfile so a later install can
    /// detect drift. An [`IndexMap`] so the user's declaration order is
    /// preserved on serialization (this map is left unsorted).
    pub overrides: Option<IndexMap<String, String>>,
    /// `ignoredOptionalDependencies` recorded the same way.
    pub ignored_optional_dependencies: Option<Vec<String>>,
    /// `patchedDependencies` recorded into the lockfile: each configured
    /// key mapped to its patch file's SHA-256 hex digest. `None` when no
    /// patches are configured.
    pub patched_dependencies: Option<BTreeMap<String, String>>,
    /// `packageExtensionsChecksum` recorded the same way. `None` when no
    /// extensions are configured (the checksum short-circuits on empty
    /// input).
    pub package_extensions_checksum: Option<String>,
    /// `pnpmfileChecksum` recorded the same way. `None` when the project
    /// has no `.pnpmfile.{cjs,mjs}` — or one that exports no `hooks`.
    pub pnpmfile_checksum: Option<String>,
    /// The workspace catalogs (with any `add` / `update` edits already
    /// merged in) used to render the lockfile's `catalogs:` snapshot —
    /// the resolved specifier + version for every `catalog:` direct
    /// dependency. Empty for projects with no catalogs.
    pub catalogs: &'a Catalogs,
    /// Default registry URL, used to decide whether a resolved registry
    /// package's tarball URL is reconstructible (and so droppable from the
    /// lockfile in favor of bare `{integrity}`).
    pub registry: &'a str,
    /// Alias → URL map of named registries (built-ins merged with the
    /// user's setting). Registry-qualified package keys route their
    /// tarball-reconstructibility check through this map instead of the
    /// default registry.
    pub registries_by_prefix: &'a HashMap<String, String>,
    pub registry_options_by_url: &'a BTreeMap<String, RegistryOptions>,
    /// When `true`, registry tarball URLs are kept in the lockfile even when
    /// reconstructible (the `lockfileIncludeTarballUrl` setting).
    pub lockfile_include_tarball_url: bool,
    /// The previous run's importer entries (the wanted lockfile's
    /// `importers:` map), keyed by the same importer ids as
    /// [`Self::importers`]. Used to preserve a workspace dependency's
    /// prior `link:` entry when this install does not target it — see
    /// `build_importer` and pnpm/pnpm#10433. `None` when there is no
    /// previous lockfile (a first install) or when `dedupeInjectedDeps`
    /// is off.
    pub previous_importers: Option<&'a HashMap<String, ProjectSnapshot>>,
    /// The previous run's `packages:` map. A rebuilt entry whose
    /// resolution is unchanged never loses a recorded `deprecated`
    /// marker to registry metadata drift. `None` on a first install.
    pub previous_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// How this install reuses the prior resolution, mapped from the
    /// `pacquet update` seed policy. Together with a spec change it
    /// decides whether an importer's workspace dependency is *targeted*
    /// by the run (and so may legitimately change its `link:`/`file:`
    /// form) — see `build_importer`. This is the workspace-wide default;
    /// [`Self::update_reuse_scopes_by_importer`] overrides it per importer.
    pub update_reuse_scope: UpdateReuseScope,
    /// Per-importer update scopes, mirroring the resolver's
    /// `update_reuse_scope_for`: a `pacquet update <name> --recursive`
    /// lowers to a `ByImporter` policy whose workspace-wide scope is `All`
    /// with the named packages recorded per importer here. The guard
    /// resolves each importer's effective scope as: global when the global
    /// is `None`, else this map's entry, else the global — so a recursive
    /// update targets the named dependency in the importer that declares
    /// it while leaving untouched importers' `link:` entries intact.
    pub update_reuse_scopes_by_importer: BTreeMap<String, UpdateReuseScope>,
    /// The lockfile's `time:` section: the prior lockfile's recorded
    /// publish dates with this run's freshly resolved ones layered over
    /// them. Empty on a first install that did not resolve `time-based`.
    /// Saving prunes it to the importers' direct dependencies.
    pub time: BTreeMap<String, String>,
}

/// Error returned while converting a resolver graph into a lockfile.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DependenciesGraphToLockfileError {
    #[diagnostic(transparent)]
    LockfileForm(#[error(source)] LockfileFormError),

    #[display(
        "Failed to serialize importer dependency {alias:?} from dependency path {dep_path:?}: {source}"
    )]
    ImporterDependency {
        alias: String,
        dep_path: String,
        #[error(source)]
        source: Box<ParseImporterDepVersionError>,
    },

    /// A resolved package whose depPath parses as no [`PackageKey`], so
    /// it can key neither `packages:` nor `snapshots:`. Every resolution
    /// but a `link:` gets a name prefixed onto its depPath — from the
    /// resolver, or failing that from the manifest the deps-resolver
    /// synthesizes — so reaching here means a resolver produced a
    /// nameless depPath. Writing the lockfile anyway would leave the
    /// importer pointing at a package neither map describes, and the
    /// install would link a dangling symlink into a virtual-store
    /// directory nothing created.
    #[display("Resolved dependency path {dep_path:?} keys no lockfile entry: {source}")]
    UnkeyedDepPath {
        dep_path: String,
        #[error(source)]
        source: Box<ParsePkgNameSuffixError<ParsePkgVerPeerError>>,
    },
}

/// Build a [`Lockfile`] from the resolver's [`DependenciesGraph`] plus
/// the per-importer context needed to populate the `importers:` map.
///
/// The output reflects pnpm v9's wire shape:
///
/// - `importers[<id>]` carries each project's `specifiers` and the
///   classified `dependencies` / `devDependencies` / `optionalDependencies`
///   maps keyed by the manifest's declared alias. The root project
///   lives under `"."`; sibling workspace projects under their POSIX
///   path from the lockfile root (e.g. `"packages/foo"`).
/// - `packages` carries one [`PackageMetadata`] entry per resolved
///   package version, keyed by the *peer-stripped* depPath (the
///   `pkgIdWithPatchHash`).
/// - `snapshots` carries one [`SnapshotEntry`](pnpm_lockfile::SnapshotEntry) per *peer-suffixed*
///   depPath — peer variants of the same package each get their own
///   snapshot row.
pub fn dependencies_graph_to_lockfile(
    opts: GraphToLockfileOptions<'_>,
) -> Result<Lockfile, DependenciesGraphToLockfileError> {
    let optional_overrides = compute_corrected_optional(&opts.importers, opts.graph);
    let (packages, snapshots) = build_packages_and_snapshots(
        opts.graph,
        &optional_overrides,
        &PackageMetadataSources {
            registry: opts.registry,
            registries_by_prefix: opts.registries_by_prefix,
            registry_options_by_url: opts.registry_options_by_url,
            lockfile_include_tarball_url: opts.lockfile_include_tarball_url,
            previous_packages: opts.previous_packages,
        },
    )?;
    let importers = build_importers(&opts)?;
    Ok(Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0))
            .expect("the generated lockfile version is supported"),
        settings: Some(LockfileSettings {
            auto_install_peers: opts.auto_install_peers,
            dedupe_peers: opts.dedupe_peers.then_some(true),
            exclude_links_from_lockfile: opts.exclude_links_from_lockfile,
            inject_workspace_packages: opts.inject_workspace_packages,
            peers_suffix_max_length: opts.peers_suffix_max_length,
        }),
        catalogs: build_catalog_snapshots(&importers, opts.catalogs),
        overrides: opts.overrides.filter(|map| !map.is_empty()),
        package_extensions_checksum: opts.package_extensions_checksum,
        pnpmfile_checksum: opts.pnpmfile_checksum,
        ignored_optional_dependencies: opts
            .ignored_optional_dependencies
            .filter(|list| !list.is_empty()),
        patched_dependencies: opts.patched_dependencies.filter(|map| !map.is_empty()),
        importers,
        packages: (!packages.is_empty()).then_some(packages),
        snapshots: (!snapshots.is_empty()).then_some(snapshots),
        time: (!opts.time.is_empty()).then_some(opts.time),
        // A freshly resolved lockfile, not a rewrite of the previous one,
        // so it starts with no foreign top-level keys. A host that records
        // its own block re-asserts it after the install (it is writing its
        // fresh contents anyway); `Lockfile::extra` is what makes that
        // read-edit-write round trip lossless.
        extra: pnpm_lockfile::LockfileExtra::default(),
    })
}

/// Build the lockfile's `catalogs:` snapshot from the resolved importers.
///
/// For every importer dependency whose recorded specifier is a `catalog:`
/// protocol, emit `{ specifier: <catalog entry>, version: <resolved> }`. The
/// `specifier` comes from `catalogs` (which already carries any `add` /
/// `update` edit), and the `version` from the importer's resolved dep map.
fn build_catalog_snapshots(
    importers: &HashMap<String, ProjectSnapshot>,
    catalogs: &Catalogs,
) -> Option<CatalogSnapshots> {
    let mut snapshots: CatalogSnapshots = BTreeMap::new();
    for importer in importers.values() {
        let Some(specifiers) = importer.specifiers.as_ref() else { continue };
        for (alias, specifier) in specifiers {
            let Some(catalog_name) = parse_catalog_protocol(specifier) else { continue };
            let Some(entry_specifier) =
                catalogs.get(catalog_name).and_then(|catalog| catalog.get(alias))
            else {
                continue;
            };
            let Some(version) = importer_resolved_version(importer, alias) else { continue };
            snapshots.entry(catalog_name.to_string()).or_default().insert(
                alias.clone(),
                ResolvedCatalogEntry { specifier: entry_specifier.clone(), version },
            );
        }
    }
    (!snapshots.is_empty()).then_some(snapshots)
}

/// Re-derive each snapshot's `optional` flag by walking the graph
/// from every importer's direct deps, classifying each starting edge
/// by the dep-group it lives in on that importer's manifest. A
/// package ends up `optional: false` iff at least one walk reached it
/// only through non-optional edges — i.e. there exists at least one
/// path from any importer to it whose edges are all non-optional.
///
/// This BFS exists because the resolver's per-node AND-fold updates
/// only the directly-revisited package, so already-walked descendants
/// stay stuck at whatever `optional` they were tagged with on the
/// first visit. See <https://github.com/pnpm/pnpm/issues/11916> for
/// the scenario.
///
/// A missing entry in the returned map means the node was never
/// reachable from any importer dep — [`build_snapshot_entry`](crate::dependencies_graph_to_lockfile::packages::build_snapshot_entry) falls
/// back to [`DependenciesGraphNode::optional`] for those, keeping an
/// untouched snapshot's existing flag.
fn compute_corrected_optional(
    importer_inputs: &BTreeMap<String, ImporterLockfileInput<'_>>,
    graph: &DependenciesGraph,
) -> HashMap<DepPath, bool> {
    // Partition every importer's deps into dev / optional / prod
    // seed sets. Across importers the union of non-optional reach is
    // what matters, so seeds are pooled before walking.
    let mut dev_seeds: Vec<&DepPath> = Vec::new();
    let mut optional_seeds: Vec<&DepPath> = Vec::new();
    let mut prod_seeds: Vec<&DepPath> = Vec::new();
    for input in importer_inputs.values() {
        let alias_to_group = manifest_alias_to_group(input.manifest);
        for (alias, dep_path) in &input.direct_dependencies_by_alias {
            // Skip aliases the manifest doesn't declare — auto-installed
            // peers hoisted into `direct_dependencies_by_alias` when
            // `autoInstallPeers: true` is on never make it into the
            // importer's lockfile entry (see [`build_importer`](crate::dependencies_graph_to_lockfile::importers::build_importer)), so we
            // don't seed from them here either. Seeding them would force
            // their snapshots' `optional` flag to `false` purely by
            // virtue of being pulled in to satisfy an optional parent's
            // peer.
            let Some(group) = alias_to_group.get(alias).copied() else {
                continue;
            };
            match group {
                DependencyGroup::Dev => dev_seeds.push(dep_path),
                DependencyGroup::Optional => optional_seeds.push(dep_path),
                DependencyGroup::Prod | DependencyGroup::Peer => prod_seeds.push(dep_path),
            }
        }
    }

    let mut walked: HashSet<(&DepPath, bool)> = HashSet::new();
    let mut visited: HashSet<&DepPath> = HashSet::new();
    let mut non_optional: HashSet<&DepPath> = HashSet::new();

    walk_subgraph(graph, &mut walked, &mut visited, &mut non_optional, dev_seeds, false);
    walk_subgraph(graph, &mut walked, &mut visited, &mut non_optional, optional_seeds, true);
    walk_subgraph(graph, &mut walked, &mut visited, &mut non_optional, prod_seeds, false);

    let mut out: HashMap<DepPath, bool> = HashMap::with_capacity(visited.len());
    for dep_path in visited {
        out.insert(dep_path.clone(), !non_optional.contains(dep_path));
    }
    out
}

/// Iterative half of [`compute_corrected_optional`]. Pushes
/// `(dep_path, optional)` pairs onto an explicit stack rather than
/// recursing so deep graphs can't blow the call stack. Children
/// declared by the parent's `optionalDependencies` (or by a
/// peer-deps-meta `optional: true`) always recurse with
/// `optional: true`; the rest inherit the parent's `optional`.
fn walk_subgraph<'g>(
    graph: &'g DependenciesGraph,
    walked: &mut HashSet<(&'g DepPath, bool)>,
    visited: &mut HashSet<&'g DepPath>,
    non_optional: &mut HashSet<&'g DepPath>,
    seeds: Vec<&'g DepPath>,
    optional: bool,
) {
    let mut stack: Vec<(&'g DepPath, bool)> = seeds.into_iter().map(|dp| (dp, optional)).collect();
    while let Some((dep_path, optional)) = stack.pop() {
        if !walked.insert((dep_path, optional)) {
            continue;
        }
        let Some(node) = graph.get(dep_path) else { continue };
        visited.insert(dep_path);
        if !optional {
            non_optional.insert(dep_path);
        }
        let opt_children = optional_children_of(node);
        for (alias, child_dep_path) in &node.children {
            let child_optional = optional || opt_children.contains(alias.as_str());
            stack.push((child_dep_path, child_optional));
        }
    }
}

/// Aliases this node treats as optional — its manifest's
/// `optionalDependencies` entries plus the names of peers marked
/// optional by `peerDependenciesMeta`.
fn optional_children_of(node: &DependenciesGraphNode) -> rustc_hash::FxHashSet<String> {
    let mut out: rustc_hash::FxHashSet<String> = node.optional_children.clone();
    if let Some(manifest) = node.resolve_result.manifest.as_ref()
        && let Some(map) = manifest.get("optionalDependencies").and_then(Value::as_object)
    {
        for name in map.keys() {
            out.insert(name.clone());
        }
    }
    for (name, peer) in &node.peer_dependencies {
        if peer.optional {
            out.insert(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests;
