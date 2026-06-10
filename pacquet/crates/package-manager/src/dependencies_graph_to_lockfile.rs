//! Adapter that converts the resolver's [`DependenciesGraph`] into a
//! [`Lockfile`].
//!
//! Ports upstream pnpm's
//! [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts)
//! plus the snapshot-vs-packages split that
//! [`convertToLockfileFile`](https://github.com/pnpm/pnpm/blob/094aa6e57b/lockfile/fs/src/lockfileFormatConverters.ts)
//! applies on write — upstream's in-memory `LockfileObject` carries one
//! merged `PackageSnapshot` per depPath which the writer fans out into
//! the v9 `packages:` + `snapshots:` pair, while pacquet's
//! [`Lockfile`] already holds the two maps separately, so this adapter
//! emits them directly.

use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;
use pacquet_lockfile::{
    CatalogSnapshots, ComVer, ImporterDepVersion, Lockfile, LockfileResolution, LockfileSettings,
    LockfileVersion, PackageKey, PackageMetadata, PeerDependencyMeta, PkgName, PkgNameVerPeer,
    PkgVerPeer, ProjectSnapshot, ResolvedCatalogEntry, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_deps_resolver::{DepPath, DependenciesGraph, DependenciesGraphNode};
use pacquet_resolving_resolver_base::ResolveResult;
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
    /// as emitted by [`pacquet_resolving_deps_resolver::resolve_peers`].
    pub direct_dependencies_by_alias: BTreeMap<String, DepPath>,
}

/// Options threaded into [`dependencies_graph_to_lockfile`].
pub struct GraphToLockfileOptions<'a> {
    /// One entry per workspace project being installed. Keyed by the
    /// lockfile importer id (`"."` for the workspace root,
    /// `"packages/<name>"` for siblings — see
    /// [`pacquet_workspace::importer_id_from_root_dir`]). Mirrors
    /// upstream's `importers: ImporterToResolve[]` shape on
    /// `resolveDependencies`.
    pub importers: BTreeMap<String, ImporterLockfileInput<'a>>,
    /// Cross-importer dedup graph keyed by `DepPath`. The fresh-resolve
    /// dispatch merges every per-importer `peers_result.graph` into
    /// this one map before calling — identical snapshot keys collapse
    /// onto one entry, matching upstream's shared
    /// [`GenericDependenciesGraph`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts#L84).
    pub graph: &'a DependenciesGraph,
    /// Round-tripped into the lockfile's top-level `settings:` block
    /// so a subsequent pnpm install can compare its own settings via
    /// `@pnpm/lockfile.settings-checker`'s `getOutdatedLockfileSetting`.
    pub auto_install_peers: bool,
    /// When `true`, the resolver ran with `dedupePeers` on and the
    /// lockfile records `dedupePeers: true` in its `settings:` block.
    /// When `false`, the key is omitted from the lockfile, mirroring
    /// pnpm's
    /// [`opts.dedupePeers || undefined`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L602)
    /// shorthand.
    pub dedupe_peers: bool,
    pub exclude_links_from_lockfile: bool,
    /// `injectWorkspacePackages` recorded the same way. Mirrors
    /// upstream's `lockfile.settings.injectWorkspacePackages`. `false`
    /// is omitted on save via [`LockfileSettings`]'s serde
    /// `skip_serializing_if`, matching
    /// [`lockfileFormatConverters.ts:70-72`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L70-L72).
    pub inject_workspace_packages: bool,
    /// `peersSuffixMaxLength` round-tripped into the lockfile's
    /// `settings.peersSuffixMaxLength` so a later install detects
    /// drift via `@pnpm/lockfile.settings-checker`. Pass `None` when
    /// the value equals upstream's default (1000) so the field is
    /// stripped from the serialized lockfile, matching upstream's
    /// [`convertToLockfileFile`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L67-L69)
    /// strip-on-default behavior.
    pub peers_suffix_max_length: Option<u64>,
    /// `overrides` recorded into the lockfile so a later install can
    /// detect drift. Mirrors upstream's `lockfile.overrides` field.
    /// An [`IndexMap`] so the user's declaration order is preserved on
    /// serialization, matching pnpm (which leaves this map unsorted).
    pub overrides: Option<IndexMap<String, String>>,
    /// `ignoredOptionalDependencies` recorded the same way.
    pub ignored_optional_dependencies: Option<Vec<String>>,
    /// `patchedDependencies` recorded into the lockfile: each configured
    /// key mapped to its patch file's SHA-256 hex digest. Mirrors
    /// upstream's `lockfile.patchedDependencies` assignment, which
    /// records [`calcPatchHashes(opts.patchedDependencies)`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L547-L549).
    /// `None` when no patches are configured.
    pub patched_dependencies: Option<BTreeMap<String, String>>,
    /// `packageExtensionsChecksum` recorded the same way. Mirrors
    /// upstream's
    /// [`packageExtensionsChecksum`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L608)
    /// assignment. `None` when no extensions are configured (matches
    /// pnpm's `hashObjectNullableWithPrefix` short-circuit on empty
    /// input).
    pub package_extensions_checksum: Option<String>,
    /// `pnpmfileChecksum` recorded the same way. Mirrors upstream's
    /// [`pnpmfileChecksum`](https://github.com/pnpm/pnpm/blob/1819226b51/installing/deps-installer/src/install/index.ts#L546)
    /// assignment. `None` when the project has no `.pnpmfile.{cjs,mjs}`
    /// — or one that exports no `hooks` — matching pnpm's
    /// `calculatePnpmfileChecksum` gate.
    pub pnpmfile_checksum: Option<String>,
    /// The workspace catalogs (with any `add` / `update` edits already
    /// merged in) used to render the lockfile's `catalogs:` snapshot —
    /// the resolved specifier + version for every `catalog:` direct
    /// dependency. Empty for projects with no catalogs.
    pub catalogs: &'a Catalogs,
    /// Default registry URL, used to decide whether a resolved registry
    /// package's tarball URL is reconstructible (and so droppable from the
    /// lockfile in favor of bare `{integrity}`). Mirrors the `registry`
    /// argument pnpm threads into
    /// [`toLockfileResolution`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/utils/src/toLockfileResolution.ts).
    pub registry: &'a str,
    /// When `true`, registry tarball URLs are kept in the lockfile even when
    /// reconstructible. Mirrors pnpm's `lockfileIncludeTarballUrl` setting.
    pub lockfile_include_tarball_url: bool,
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
/// - `snapshots` carries one [`SnapshotEntry`] per *peer-suffixed*
///   depPath — peer variants of the same package each get their own
///   snapshot row.
#[must_use]
pub fn dependencies_graph_to_lockfile(opts: GraphToLockfileOptions<'_>) -> Lockfile {
    let GraphToLockfileOptions {
        importers: importer_inputs,
        graph,
        auto_install_peers,
        dedupe_peers,
        exclude_links_from_lockfile,
        inject_workspace_packages,
        peers_suffix_max_length,
        overrides,
        ignored_optional_dependencies,
        patched_dependencies,
        package_extensions_checksum,
        pnpmfile_checksum,
        catalogs,
        registry,
        lockfile_include_tarball_url,
    } = opts;

    let optional_overrides = compute_corrected_optional(&importer_inputs, graph);
    let (packages, snapshots) = build_packages_and_snapshots(
        graph,
        &optional_overrides,
        registry,
        lockfile_include_tarball_url,
    );

    let mut importers: HashMap<String, ProjectSnapshot> =
        HashMap::with_capacity(importer_inputs.len());
    for (id, input) in &importer_inputs {
        importers.insert(id.clone(), build_importer(input, graph, exclude_links_from_lockfile));
    }

    let catalog_snapshots = build_catalog_snapshots(&importers, catalogs);

    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0))
            .expect("lockfileVersion 9.0 is always compatible with MAJOR=9"),
        settings: Some(LockfileSettings {
            auto_install_peers,
            dedupe_peers: dedupe_peers.then_some(true),
            exclude_links_from_lockfile,
            inject_workspace_packages,
            peers_suffix_max_length,
        }),
        catalogs: catalog_snapshots,
        overrides: overrides.filter(|map| !map.is_empty()),
        package_extensions_checksum,
        pnpmfile_checksum,
        ignored_optional_dependencies: ignored_optional_dependencies
            .filter(|list| !list.is_empty()),
        patched_dependencies: patched_dependencies.filter(|map| !map.is_empty()),
        importers,
        packages: (!packages.is_empty()).then_some(packages),
        snapshots: (!snapshots.is_empty()).then_some(snapshots),
    }
}

/// Build the lockfile's `catalogs:` snapshot from the resolved importers.
///
/// Ports pnpm's
/// [`getCatalogSnapshots`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/installing/deps-resolver/src/getCatalogSnapshots.ts):
/// for every importer dependency whose recorded specifier is a `catalog:`
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

/// The concrete version `alias` resolved to in `importer`, read from whichever
/// dependency group carries it. Returns the peer-stripped version, matching
/// pnpm's `dep.version` in a catalog snapshot.
///
/// Uses [`ImporterDepVersion::ver_peer`] rather than `as_regular` so a catalog
/// entry resolved through an `npm:` alias (e.g. `js-yaml: npm:@zkochan/js-yaml@0.0.11`,
/// stored as [`ImporterDepVersion::Alias`]) still records its version (`0.0.11`)
/// — `as_regular` returns `None` for aliases, which silently dropped aliased
/// catalog entries from the `catalogs:` snapshot.
fn importer_resolved_version(importer: &ProjectSnapshot, alias: &str) -> Option<String> {
    let key = PkgName::parse(alias).ok()?;
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .find_map(|map| map.get(&key))
        .and_then(|spec| spec.version.ver_peer())
        .map(|version| version.version().to_string())
}

/// Build an importer's [`ProjectSnapshot`] from its on-disk manifest
/// plus the per-alias `DepPath` map the resolver produced for that
/// importer.
///
/// Mirrors upstream's
/// [`addDirectDependenciesToLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts#L417-L484):
/// the manifest decides which dep group each alias lives under, and the
/// resolver decides the resolved version (peer-suffixed when peers are
/// involved, alias-prefixed when the alias and real name differ).
///
/// When `exclude_links_from_lockfile` is `true`, a `link:` direct
/// dependency is omitted from the importer's `specifiers` and
/// `dependencies` / `devDependencies` / `optionalDependencies` maps
/// — unless its manifest specifier starts with `workspace:`, which
/// still records the resolved workspace-sibling target so the
/// lockfile stays a complete description of the workspace graph.
/// Mirrors upstream's
/// [exclude-link gate](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L449-L456).
fn build_importer(
    input: &ImporterLockfileInput<'_>,
    graph: &DependenciesGraph,
    exclude_links_from_lockfile: bool,
) -> ProjectSnapshot {
    let manifest = input.manifest;
    let direct = &input.direct_dependencies_by_alias;

    let mut dependencies: ResolvedDependencyMap = HashMap::new();
    let mut dev_dependencies: ResolvedDependencyMap = HashMap::new();
    let mut optional_dependencies: ResolvedDependencyMap = HashMap::new();
    let mut specifiers: HashMap<String, String> = HashMap::new();

    let alias_to_group = manifest_alias_to_group(manifest);

    for (alias, dep_path) in direct {
        let Ok(name_for_key) = PkgName::parse(alias.as_str()) else { continue };
        // Skip aliases the manifest doesn't declare. The resolver's
        // `direct_dependencies_by_alias` includes auto-installed peers
        // hoisted to the importer when `autoInstallPeers: true` is on,
        // but pnpm's `addDirectDependenciesToLockfile` iterates only
        // over `getAllDependenciesFromManifest(manifest)` — transitive
        // auto-installed peers never enter `importer.dependencies` /
        // `importer.specifiers`, only the snapshots graph below. Writing
        // them here would carry specifiers the manifest can't satisfy
        // through `satisfies_package_manifest` and force every later
        // install onto the fresh-resolve path.
        let Some(specifier) = read_manifest_specifier(manifest, alias) else { continue };
        // Workspace-link nodes don't enter the graph (the resolver
        // short-circuits them at `depth = -1`); resolve the importer
        // version directly from the `link:` depPath instead. Non-link
        // direct deps must be present in the graph — a missing entry
        // means the resolver dropped the edge, so skip.
        let version = if let Some(target) = dep_path.as_str().strip_prefix("link:") {
            if exclude_links_from_lockfile && !specifier.starts_with("workspace:") {
                continue;
            }
            ImporterDepVersion::Link(target.to_string())
        } else {
            let Some(node) = graph.get(dep_path) else { continue };
            importer_dep_version(alias, node)
        };
        let spec = ResolvedDependencySpec { specifier: specifier.clone(), version };
        specifiers.insert(alias.clone(), specifier);
        let group = alias_to_group.get(alias).copied().unwrap_or(DependencyGroup::Prod);
        match group {
            DependencyGroup::Dev => {
                dev_dependencies.insert(name_for_key, spec);
            }
            DependencyGroup::Optional => {
                optional_dependencies.insert(name_for_key, spec);
            }
            DependencyGroup::Prod | DependencyGroup::Peer => {
                dependencies.insert(name_for_key, spec);
            }
        }
    }

    ProjectSnapshot {
        specifiers: (!specifiers.is_empty()).then_some(specifiers),
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        dev_dependencies: (!dev_dependencies.is_empty()).then_some(dev_dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        dependencies_meta: None,
        publish_directory: None,
    }
}

/// Map each direct-dep alias to the manifest group it appears in.
/// `dependencies` wins over `devDependencies` wins over
/// `optionalDependencies` when an alias is duplicated across groups —
/// mirrors upstream's
/// [`getAliasToDependencyTypeMap`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts#L500-L511)
/// (first-write-wins over `DEPENDENCIES_FIELDS`).
fn manifest_alias_to_group(manifest: &PackageManifest) -> HashMap<String, DependencyGroup> {
    let mut out: HashMap<String, DependencyGroup> = HashMap::new();
    for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        for (alias, _) in manifest.dependencies([group]) {
            out.entry(alias.to_string()).or_insert(group);
        }
    }
    out
}

/// Look up the user-written specifier for `alias` in the manifest's
/// `dependencies` / `devDependencies` / `optionalDependencies` /
/// `peerDependencies` maps in pnpm's
/// [`DEPENDENCIES_FIELDS`](https://github.com/pnpm/pnpm/blob/097983fbca/packages/types/src/misc.ts)
/// precedence order. Returns `None` for a peer-only entry that was
/// auto-installed but isn't recorded as a direct dep in the manifest —
/// such entries don't go into the importer's `specifiers` map.
fn read_manifest_specifier(manifest: &PackageManifest, alias: &str) -> Option<String> {
    for group in [
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ] {
        let group_key: &str = group.into();
        if let Some(map) = manifest.value().get(group_key).and_then(Value::as_object)
            && let Some(spec) = map.get(alias).and_then(Value::as_str)
        {
            return Some(spec.to_string());
        }
    }
    None
}

/// Build the version cell for an importer-level dependency, mirroring
/// pnpm's [`depPathToRef`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/depPathToRef.ts):
///
/// - When the depPath is a workspace-link id (`link:<rel-path>`), emit
///   the [`ImporterDepVersion::Link`] arm so the lockfile records the
///   sibling project's relative path instead of trying to parse it as
///   `name@version`.
/// - When the depPath is an injected workspace id (`file:<rel-path>`
///   plus optional `(peer@suffix)`), emit the
///   [`ImporterDepVersion::File`] arm so the importer entry records
///   the `file:` snapshot key instead of trying to parse it as
///   `name@version`. The injected workspace dep didn't dedupe back to
///   `link:` because its children weren't a subset of the target
///   project's direct deps (or `dedupeInjectedDeps` is off).
/// - When the resolved real name equals the manifest alias and the
///   depPath starts with `<name>@`, drop the prefix so the importer
///   carries just the version-with-peer string.
/// - Otherwise — npm-alias entries, where the alias and real name
///   differ — keep the full `<real>@<version-with-peer>` string so the
///   snapshot key the importer points at is unambiguous.
fn importer_dep_version(alias: &str, node: &DependenciesGraphNode) -> ImporterDepVersion {
    let dep_path_str = node.dep_path.as_str();

    if let Some(target) = dep_path_str.strip_prefix("link:") {
        return ImporterDepVersion::Link(target.to_string());
    }
    if let Some(target) = dep_path_str.strip_prefix("file:") {
        return ImporterDepVersion::File(target.to_string());
    }

    let real_name = real_name(&node.resolve_result);
    if let Some(real) = real_name.as_deref() {
        let prefix = format!("{real}@");
        if alias == real
            && let Some(ver) = dep_path_str.strip_prefix(&prefix)
            && let Ok(parsed) = ver.parse::<PkgVerPeer>()
        {
            return ImporterDepVersion::Regular(parsed);
        }
    }
    dep_path_str
        .parse::<PkgNameVerPeer>()
        .map(ImporterDepVersion::Alias)
        .expect("dep paths produced by the resolver always parse as PkgNameVerPeer")
}

/// `Some(real_name)` when the resolver produced a structured name; `None`
/// for resolvers that learn the name from the fetched manifest (git,
/// tarball, file). Matches the fallback path
/// [`pkg_name_version`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L23-L37)
/// uses in the resolve-peers walker — and the same shape the
/// `name_ver` field carries upstream.
fn real_name(result: &ResolveResult) -> Option<String> {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return Some(name_ver.name.to_string());
    }
    // `name_ver` is unset for resolutions that learn the canonical name
    // from the fetched manifest. Read it for the two shapes whose `name@`
    // prefix pnpm's `depPathToRef` strips off the importer entry:
    // - a remote (non-registry, non-git) http(s) tarball direct dep
    //   (`<name>@<tarball-url>` -> `version: <url>`), and
    // - a runtime dep (`<name>@runtime:<ver>`, a Variations resolution ->
    //   `version: runtime:<ver>`).
    // Other manifest-only resolutions (`file:` / git) are deliberately
    // left to the `None` path so their importer entries keep pacquet's
    // current prefixed shape — bringing those in line is separate from
    // <https://github.com/pnpm/pnpm/issues/12053>.
    let reads_name_from_manifest = match &result.resolution {
        LockfileResolution::Variations(_) => true,
        LockfileResolution::Tarball(tarball) => {
            tarball.git_hosted != Some(true) && is_remote_http_tarball(&tarball.tarball)
        }
        _ => false,
    };
    if !reads_name_from_manifest {
        return None;
    }
    result.manifest.as_ref()?.get("name")?.as_str().map(str::to_string)
}

/// `true` for an `http(s)://` tarball URL — the remote tarball deps
/// covered by <https://github.com/pnpm/pnpm/issues/12053>. Excludes
/// `file:` tarballs and registry-reconstructed resolutions that carry
/// no URL.
fn is_remote_http_tarball(tarball: &str) -> bool {
    tarball.starts_with("http:") || tarball.starts_with("https:")
}

/// Walk the depPath-keyed [`DependenciesGraph`] and emit the matching
/// `(PackageMetadata, SnapshotEntry)` pair for each node — fanned out
/// across the two top-level maps the v9 lockfile splits.
///
/// Multiple snapshot entries (peer variants) share one packages entry,
/// so the loop dedupes by peer-stripped key.
///
/// `optional_overrides` carries the corrected `optional` flag per
/// depPath produced by [`compute_corrected_optional`]; a missing
/// entry falls back to [`DependenciesGraphNode::optional`].
fn build_packages_and_snapshots(
    graph: &DependenciesGraph,
    optional_overrides: &HashMap<DepPath, bool>,
    registry: &str,
    lockfile_include_tarball_url: bool,
) -> (HashMap<PackageKey, PackageMetadata>, HashMap<PackageKey, SnapshotEntry>) {
    let mut packages: HashMap<PackageKey, PackageMetadata> = HashMap::new();
    let mut snapshots: HashMap<PackageKey, SnapshotEntry> = HashMap::new();

    for node in graph.values() {
        let Ok(snapshot_key) = node.dep_path.as_str().parse::<PackageKey>() else { continue };
        let metadata_key = snapshot_key.without_peer();

        let snapshot = build_snapshot_entry(node, graph, optional_overrides);
        snapshots.insert(snapshot_key, snapshot);

        packages.entry(metadata_key).or_insert_with_key(|key| {
            build_package_metadata(node, key, registry, lockfile_include_tarball_url)
        });
    }

    (packages, snapshots)
}

/// Build the per-`(name, version)` [`PackageMetadata`] block for the
/// lockfile's `packages:` map. Pulls `engines` / `cpu` / `os` / `libc` /
/// `deprecated` / `hasBin` / `bundledDependencies` / `peerDependencies`
/// off the resolver's manifest fragment when present.
///
/// Mirrors the field-by-field copy in
/// [`toLockfileDependency`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L49-L156)
/// for the per-package half (excludes the per-snapshot fields
/// `dependencies` / `optionalDependencies` / `transitivePeerDependencies` /
/// `optional` / `patched`, which go on the snapshot below).
fn build_package_metadata(
    node: &DependenciesGraphNode,
    metadata_key: &PackageKey,
    registry: &str,
    lockfile_include_tarball_url: bool,
) -> PackageMetadata {
    let manifest = node.resolve_result.manifest.as_deref();

    let engines = manifest
        .and_then(|m| m.get("engines"))
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(name, value)| {
                    let range = value.as_str()?;
                    if range == "*" {
                        return None;
                    }
                    Some((name.clone(), range.to_string()))
                })
                .collect::<HashMap<String, String>>()
        })
        .filter(|map| !map.is_empty());

    let cpu = read_string_list(manifest, "cpu");
    let os = read_string_list(manifest, "os");
    let libc = read_string_list(manifest, "libc");

    let deprecated =
        manifest.and_then(|m| m.get("deprecated")).and_then(Value::as_str).map(ToString::to_string);

    let has_bin = manifest_has_bin(manifest);

    let bundled_dependencies = read_string_list(manifest, "bundledDependencies")
        .or_else(|| read_string_list(manifest, "bundleDependencies"));

    let (peer_dependencies, peer_dependencies_meta) = build_peer_dep_blocks(node);

    let resolution = node.resolve_result.resolution.to_lockfile_form(
        &metadata_key.name.to_string(),
        &metadata_key.suffix.version().to_string(),
        registry,
        lockfile_include_tarball_url,
    );

    // pnpm records `version` only for non-registry packages (depPath carries
    // a `:`), and only when the manifest declares one and the resolution
    // isn't a local directory — see `toLockfileDependency`. Registry packages
    // omit it because their version is already the depPath suffix.
    let version = (node.dep_path.as_str().contains(':')
        && !matches!(resolution, LockfileResolution::Directory(_)))
    .then(|| {
        manifest.and_then(|m| m.get("version")).and_then(Value::as_str).map(ToString::to_string)
    })
    .flatten();

    PackageMetadata {
        resolution,
        version,
        engines,
        cpu,
        os,
        libc,
        deprecated,
        has_bin,
        prepare: None,
        bundled_dependencies,
        peer_dependencies,
        peer_dependencies_meta,
    }
}

/// Read a JSON array field off the resolver's manifest fragment and
/// flatten it into a `Vec<String>`. `None` when the field is missing or
/// not an array of strings — matches upstream's silent drop for
/// malformed metadata.
fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    let arr = manifest?.get(key)?.as_array()?;
    let out: Vec<String> = arr.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
    (!out.is_empty()).then_some(out)
}

/// `Some(true)` when the manifest declares a `bin` entry (string or
/// non-empty object map). Pacquet records the same `hasBin: true`
/// signal pnpm writes; the field is dropped entirely when absent.
fn manifest_has_bin(manifest: Option<&Value>) -> Option<bool> {
    let value = manifest?.get("bin")?;
    let present = match value {
        Value::String(s) => !s.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => false,
    };
    present.then_some(true)
}

/// Returned `Option`-pair from [`build_peer_dep_blocks`]: the
/// `peerDependencies` map (name → range) and the
/// `peerDependenciesMeta` map (name → `{ optional: true }`).
type PeerDepBlocks = (Option<HashMap<String, String>>, Option<HashMap<String, PeerDependencyMeta>>);

/// Split the resolver's `peer_dependencies` into the
/// `peerDependencies` (name → range) and `peerDependenciesMeta`
/// (name → `{ optional: true }`) blocks pnpm writes onto `packages:`.
fn build_peer_dep_blocks(node: &DependenciesGraphNode) -> PeerDepBlocks {
    if node.peer_dependencies.is_empty() {
        return (None, None);
    }
    let mut peers: HashMap<String, String> = HashMap::new();
    let mut peers_meta: HashMap<String, PeerDependencyMeta> = HashMap::new();
    for (name, peer) in &node.peer_dependencies {
        peers.insert(name.clone(), peer.version.clone());
        if peer.optional {
            peers_meta.insert(name.clone(), PeerDependencyMeta { optional: true });
        }
    }
    let peers_meta = (!peers_meta.is_empty()).then_some(peers_meta);
    (Some(peers), peers_meta)
}

/// Build the per-snapshot [`SnapshotEntry`] for this depPath. Mirrors
/// the per-snapshot half of upstream's
/// [`toLockfileDependency`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L49-L156):
/// the `dependencies` / `optionalDependencies` partition follows the
/// node's own `optionalDependencies` set and peer-optional flag;
/// `transitivePeerDependencies` is sorted; `optional` is sourced from
/// [`compute_corrected_optional`] (the lockfile-pruner BFS port,
/// which re-derives the flag from the importer graph because the
/// resolver's per-node fold misses transitive descendants on
/// revisits — see <https://github.com/pnpm/pnpm/issues/11916>).
/// `BuildModules` consults this flag to decide whether a build
/// failure is fatal or should be reported via
/// `pnpm:skipped-optional-dependency`.
fn build_snapshot_entry(
    node: &DependenciesGraphNode,
    graph: &DependenciesGraph,
    optional_overrides: &HashMap<DepPath, bool>,
) -> SnapshotEntry {
    let optional_children = optional_children_of(node);

    let mut dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    let mut optional_dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (alias, child_dep_path) in &node.children {
        let Ok(alias_name) = PkgName::parse(alias.as_str()) else { continue };
        let Some(child_ref) = snapshot_dep_ref(alias, child_dep_path, graph) else { continue };
        if optional_children.contains(alias.as_str()) {
            optional_dependencies.insert(alias_name, child_ref);
        } else {
            dependencies.insert(alias_name, child_ref);
        }
    }

    let transitive: Vec<String> = {
        let mut list: Vec<String> = node.transitive_peer_dependencies.iter().cloned().collect();
        list.sort();
        list
    };

    let optional = optional_overrides.get(&node.dep_path).copied().unwrap_or(node.optional);

    SnapshotEntry {
        id: None,
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        transitive_peer_dependencies: (!transitive.is_empty()).then_some(transitive),
        patched: None,
        optional,
    }
}

/// Re-derive each snapshot's `optional` flag by walking the graph
/// from every importer's direct deps, classifying each starting edge
/// by the dep-group it lives in on that importer's manifest. A
/// package ends up `optional: false` iff at least one walk reached it
/// only through non-optional edges — i.e. there exists at least one
/// path from any importer to it whose edges are all non-optional.
///
/// Ports upstream pnpm's
/// [`copyDependencySubGraph`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/lockfile/pruner/src/index.ts#L160-L205)
/// BFS, which exists for exactly this reason: the resolver's
/// per-node AND-fold at
/// [`resolveDependencies.ts:1630`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencies.ts#L1627-L1648)
/// updates only the directly-revisited package, so the
/// already-walked descendants stay stuck at whatever `optional` they
/// were tagged with on the first visit. See
/// <https://github.com/pnpm/pnpm/issues/11916> for the scenario.
///
/// A missing entry in the returned map means the node was never
/// reachable from any importer dep — [`build_snapshot_entry`] falls
/// back to [`DependenciesGraphNode::optional`] for those, matching
/// upstream's "untouched depLockfile keeps its existing flag" arm.
fn compute_corrected_optional(
    importer_inputs: &BTreeMap<String, ImporterLockfileInput<'_>>,
    graph: &DependenciesGraph,
) -> HashMap<DepPath, bool> {
    // Partition every importer's deps by group, mirroring the
    // `(devDepPaths, optionalDepPaths, prodDepPaths)` split upstream
    // hands to `copyDependencySubGraph`. Across importers the union
    // of non-optional reach is what matters, so seeds are pooled
    // before walking.
    let mut dev_seeds: Vec<&DepPath> = Vec::new();
    let mut optional_seeds: Vec<&DepPath> = Vec::new();
    let mut prod_seeds: Vec<&DepPath> = Vec::new();
    for input in importer_inputs.values() {
        let alias_to_group = manifest_alias_to_group(input.manifest);
        for (alias, dep_path) in &input.direct_dependencies_by_alias {
            // Skip aliases the manifest doesn't declare — auto-installed
            // peers hoisted into `direct_dependencies_by_alias` when
            // `autoInstallPeers: true` is on never make it into the
            // importer's lockfile entry (see [`build_importer`]), so
            // upstream's [`pruneSharedLockfile`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/lockfile/pruner/src/index.ts#L27-L29)
            // doesn't seed from them either. Seeding them here would
            // force their snapshots' `optional` flag to `false` purely
            // by virtue of being pulled in to satisfy an optional
            // parent's peer.
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
/// optional by `peerDependenciesMeta`. Mirrors upstream's `partition`
/// over `(child) => depNode.optionalDependencies.has(child.alias) ||
/// depNode.peerDependencies[child.alias]?.optional === true` in
/// [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L28-L31).
fn optional_children_of(node: &DependenciesGraphNode) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
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

/// Build the `<alias>: <ref>` value the snapshot writes per child edge.
/// Mirrors importer-side [`importer_dep_version`]: the `link:` branch
/// emits [`SnapshotDepRef::Link`] for workspace siblings, plain /
/// alias otherwise.
fn snapshot_dep_ref(
    alias: &str,
    child_dep_path: &DepPath,
    graph: &DependenciesGraph,
) -> Option<SnapshotDepRef> {
    let dep_path_str = child_dep_path.as_str();
    if let Some(target) = dep_path_str.strip_prefix("link:") {
        return Some(SnapshotDepRef::Link(target.to_string()));
    }
    let real_name = graph.get(child_dep_path).and_then(|n| real_name(&n.resolve_result));
    if let Some(real) = real_name.as_deref() {
        let prefix = format!("{real}@");
        if alias == real
            && let Some(ver) = dep_path_str.strip_prefix(&prefix)
            && let Ok(parsed) = ver.parse::<PkgVerPeer>()
        {
            return Some(SnapshotDepRef::Plain(parsed));
        }
    }
    dep_path_str.parse::<PkgNameVerPeer>().ok().map(SnapshotDepRef::Alias)
}

#[cfg(test)]
mod tests;
