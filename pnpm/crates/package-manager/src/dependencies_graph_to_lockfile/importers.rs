use super::{
    DependenciesGraphToLockfileError, GraphToLockfileOptions, ImporterLockfileFlags,
    ImporterLockfileInput,
};
use pnpm_lockfile::{
    ImporterDepVersion, LockfileResolution, ParseImporterDepVersionError, PkgName, PkgNameVerPeer,
    PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, VersionPart,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{
    DepPath, DependenciesGraph, DependenciesGraphNode, UpdateReuseScope,
};
use pnpm_resolving_resolver_base::ResolveResult;
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

/// Each importer's snapshot reads only its own input plus the shared
/// graph, so a workspace-scale importer list fans out across the
/// rayon pool; the serial fold below keeps the first error in
/// importer order.
pub(super) fn build_importers(
    opts: &GraphToLockfileOptions<'_>,
) -> Result<HashMap<String, ProjectSnapshot>, DependenciesGraphToLockfileError> {
    let importer_inputs: Vec<_> = opts.importers.iter().collect();
    let importer_results: Vec<(
        &String,
        Result<ProjectSnapshot, DependenciesGraphToLockfileError>,
    )> = importer_inputs
        .par_iter()
        .map(|&(id, input)| {
            let importer = build_importer(
                input,
                opts.graph,
                &ImporterLockfileFlags {
                    exclude_links_from_lockfile: opts.exclude_links_from_lockfile,
                    auto_install_peers: opts.auto_install_peers,
                },
                opts.previous_importers.and_then(|importers| importers.get(id)),
                effective_update_reuse_scope(opts, id),
            );
            (id, importer)
        })
        .collect();
    let mut importers: HashMap<String, ProjectSnapshot> =
        HashMap::with_capacity(opts.importers.len());
    for (id, importer) in importer_results {
        importers.insert(id.clone(), importer?);
    }
    Ok(importers)
}
/// Effective update scope for this importer, mirroring the resolver's
/// `update_reuse_scope_for`: a global `None` (bare `update`) applies to
/// every importer; otherwise the per-importer entry wins, falling back
/// to the global. This is what lets a `pacquet update <name> --recursive`
/// target the named dependency in the importer that declares it while
/// leaving untouched importers on their global scope.
pub(super) fn effective_update_reuse_scope<'o>(
    opts: &'o GraphToLockfileOptions<'_>,
    importer_id: &str,
) -> &'o UpdateReuseScope {
    if matches!(opts.update_reuse_scope, UpdateReuseScope::None) {
        &opts.update_reuse_scope
    } else {
        opts.update_reuse_scopes_by_importer.get(importer_id).unwrap_or(&opts.update_reuse_scope)
    }
}
/// The concrete version `alias` resolved to in `importer`, read from whichever
/// dependency group carries it. Returns the peer-stripped version recorded as
/// the `version` in a catalog snapshot.
pub(super) fn importer_resolved_version(importer: &ProjectSnapshot, alias: &str) -> Option<String> {
    let key = PkgName::parse(alias).ok()?;
    let groups =
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
    let spec = groups.into_iter().flatten().find_map(|map| map.get(&key))?;
    spec.version.ver_peer().map(|version| version.version().to_string())
}
/// Build an importer's [`ProjectSnapshot`] from its on-disk manifest
/// plus the per-alias `DepPath` map the resolver produced for that
/// importer.
///
/// The manifest decides which dep group each alias lives under, and the
/// resolver decides the resolved version (peer-suffixed when peers are
/// involved, alias-prefixed when the alias and real name differ).
///
/// When `exclude_links_from_lockfile` is `true`, a `link:` direct
/// dependency is omitted from the importer's `specifiers` and
/// `dependencies` / `devDependencies` / `optionalDependencies` maps
/// — unless its manifest specifier starts with `workspace:`, which
/// still records the resolved workspace-sibling target so the
/// lockfile stays a complete description of the workspace graph.
pub(super) fn build_importer(
    input: &ImporterLockfileInput<'_>,
    graph: &DependenciesGraph,
    flags: &ImporterLockfileFlags,
    previous_importer: Option<&ProjectSnapshot>,
    update_reuse_scope: &UpdateReuseScope,
) -> Result<ProjectSnapshot, DependenciesGraphToLockfileError> {
    let mut groups = ImporterDependencyGroups::default();
    let mut specifiers: HashMap<String, String> = HashMap::new();
    let alias_to_group = manifest_alias_to_group(input.manifest);
    let sources = DirectEntrySources {
        manifest: input.manifest,
        graph,
        flags,
        previous_importer,
        update_reuse_scope,
    };
    for (alias, dep_path) in &input.direct_dependencies_by_alias {
        let Some((name_for_key, spec)) = importer_direct_entry(alias, dep_path, &sources)? else {
            continue;
        };
        specifiers.insert(alias.clone(), spec.specifier.clone());
        groups.insert(
            alias_to_group.get(alias).copied().unwrap_or(DependencyGroup::Prod),
            name_for_key,
            spec,
        );
    }
    let (publish_directory, link_directory) = manifest_publish_config(input.manifest);
    Ok(ProjectSnapshot {
        specifiers: (!specifiers.is_empty()).then_some(specifiers),
        dependencies: (!groups.prod.is_empty()).then_some(groups.prod),
        dev_dependencies: (!groups.dev.is_empty()).then_some(groups.dev),
        optional_dependencies: (!groups.optional.is_empty()).then_some(groups.optional),
        dependencies_meta: input
            .manifest
            .value()
            .get("dependenciesMeta")
            .filter(|value| value.as_object().is_some_and(|meta| !meta.is_empty()))
            .cloned(),
        publish_directory,
        link_directory,
    })
}
/// What every direct dependency's importer entry is read against.
pub(super) struct DirectEntrySources<'a> {
    manifest: &'a PackageManifest,
    graph: &'a DependenciesGraph,
    flags: &'a ImporterLockfileFlags,
    previous_importer: Option<&'a ProjectSnapshot>,
    update_reuse_scope: &'a UpdateReuseScope,
}
/// One direct dependency's importer entry: the key it is recorded under and
/// its specifier and resolved version. `None` leaves the alias out of the
/// importer.
pub(super) fn importer_direct_entry(
    alias: &str,
    dep_path: &DepPath,
    sources: &DirectEntrySources<'_>,
) -> Result<Option<(PkgName, ResolvedDependencySpec)>, DependenciesGraphToLockfileError> {
    let Ok(name_for_key) = PkgName::parse(alias) else { return Ok(None) };
    // Skip aliases the manifest doesn't declare. The resolver's
    // `direct_dependencies_by_alias` includes auto-installed peers
    // hoisted to the importer when `autoInstallPeers: true` is on,
    // but only aliases declared in the manifest belong in the
    // importer entry — transitive auto-installed peers never enter
    // `importer.dependencies` / `importer.specifiers`, only the
    // snapshots graph below. Writing them here would carry specifiers
    // the manifest can't satisfy through `satisfies_package_manifest`
    // and force every later install onto the fresh-resolve path.
    let Some(specifier) =
        read_manifest_specifier(sources.manifest, alias, sources.flags.auto_install_peers)
    else {
        return Ok(None);
    };
    let Some(version) = direct_dep_version(
        alias,
        dep_path,
        &specifier,
        sources.graph,
        sources.flags.exclude_links_from_lockfile,
    )?
    else {
        return Ok(None);
    };
    let version = preserved_link_version(
        &version,
        &PreservedLinkLookup {
            previous_importer: sources.previous_importer,
            name_for_key: &name_for_key,
            specifier: &specifier,
            dep_path,
            graph: sources.graph,
            update_reuse_scope: sources.update_reuse_scope,
        },
    )
    .unwrap_or(version);
    Ok(Some((name_for_key, ResolvedDependencySpec { specifier, version })))
}
/// One importer's direct dependencies, split by the manifest group they were
/// declared in.
#[derive(Default)]
pub(super) struct ImporterDependencyGroups {
    prod: ResolvedDependencyMap,
    dev: ResolvedDependencyMap,
    optional: ResolvedDependencyMap,
}
impl ImporterDependencyGroups {
    fn insert(&mut self, group: DependencyGroup, name: PkgName, spec: ResolvedDependencySpec) {
        match group {
            DependencyGroup::Dev => self.dev.insert(name, spec),
            DependencyGroup::Optional => self.optional.insert(name, spec),
            DependencyGroup::Prod | DependencyGroup::Peer => self.prod.insert(name, spec),
        };
    }
}
/// The version one direct dependency records, or `None` when the entry does
/// not belong in the importer.
///
/// Workspace-link nodes don't enter the graph (the resolver short-circuits
/// them at `depth = -1`), so the importer version comes straight from the
/// `link:` depPath. Non-link direct deps must be present in the graph — a
/// missing entry means the resolver dropped the edge.
pub(super) fn direct_dep_version(
    alias: &str,
    dep_path: &DepPath,
    specifier: &str,
    graph: &DependenciesGraph,
    exclude_links_from_lockfile: bool,
) -> Result<Option<ImporterDepVersion>, DependenciesGraphToLockfileError> {
    if let Some(target) = dep_path.as_str().strip_prefix("link:") {
        if exclude_links_from_lockfile && !specifier.starts_with("workspace:") {
            return Ok(None);
        }
        return Ok(Some(ImporterDepVersion::Link(target.to_string())));
    }
    let Some(node) = graph.get(dep_path) else { return Ok(None) };
    importer_dep_version(alias, node).map(Some).map_err(|source| {
        DependenciesGraphToLockfileError::ImporterDependency {
            alias: alias.to_string(),
            dep_path: dep_path.to_string(),
            source: Box::new(source),
        }
    })
}
/// What [`preserved_link_version`] consults to decide whether this install
/// targets the dependency.
pub(super) struct PreservedLinkLookup<'a> {
    previous_importer: Option<&'a ProjectSnapshot>,
    name_for_key: &'a PkgName,
    specifier: &'a str,
    dep_path: &'a DepPath,
    graph: &'a DependenciesGraph,
    update_reuse_scope: &'a UpdateReuseScope,
}
/// pnpm/pnpm#10433: a fresh-lockfile install re-resolves every importer, and
/// an injected workspace dependency whose peer context genuinely diverges
/// reaches [`importer_dep_version`]'s `file:` arm instead of deduping back to
/// `link:`. When this install does not *target* that dependency, its previous
/// `link:` importer entry is kept rather than rewritten to a peer-suffixed
/// `file:`. `dedupe_injected_deps` runs earlier in the resolver and does not
/// reach this finalization path.
///
/// A workspace dependency the run doesn't target keeps its `link:`. It is
/// targeted when this importer's update scope names it (`pacquet update
/// <name>`, including the per-importer scope of a `--recursive` run), when the
/// scope is `None` (a scope-wide bare `update` / forced re-resolve), or when
/// its specifier changed (a new or edited manifest entry). `KeepAll` (plain
/// install / add) never targets on its own, so an untouched workspace dep is
/// preserved. `update_reuse_scope` here is already resolved for this importer
/// (see `update_reuse_scope_for` in the caller), so `pacquet update <name>
/// --recursive` targets the named dep in the importer that declares it while
/// untouched importers keep their `link:`. Matches the TS resolver's
/// `updateTargetedAliases` / `updateMatching` guard, where a plain install's
/// blanket spec re-check must not count as targeting.
pub(super) fn preserved_link_version(
    version: &ImporterDepVersion,
    lookup: &PreservedLinkLookup<'_>,
) -> Option<ImporterDepVersion> {
    let ImporterDepVersion::File(_) = version else { return None };
    let previous = lookup
        .previous_importer
        .and_then(|prev| previous_importer_dep(prev, lookup.name_for_key))?;
    let ImporterDepVersion::Link(_) = &previous.version else { return None };
    let targeted_by_update = match lookup.update_reuse_scope {
        UpdateReuseScope::All => false,
        UpdateReuseScope::None => true,
        // By name alone: this runs after resolution, where the
        // version in hand is the one the update just produced, not
        // the line the selector asked to move.
        UpdateReuseScope::Except(targets) => lookup
            .graph
            .get(lookup.dep_path)
            .and_then(node_pkg_name)
            .is_some_and(|name| targets.covers(&name, None)),
    };
    let targeted_by_spec_change = previous.specifier != lookup.specifier;
    (!targeted_by_update && !targeted_by_spec_change).then(|| previous.version.clone())
}
pub(crate) fn manifest_publish_config(
    manifest: &PackageManifest,
) -> (Option<String>, Option<bool>) {
    let publish_config = manifest.value().get("publishConfig");
    let publish_directory = publish_config
        .and_then(|publish_config| publish_config.get("directory"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let link_directory = publish_directory.as_ref().and_then(|_| {
        publish_config
            .and_then(|publish_config| publish_config.get("linkDirectory"))
            .and_then(Value::as_bool)
            .filter(|link_directory| !link_directory)
    });
    (publish_directory, link_directory)
}
/// Map each direct-dep alias to the manifest group it appears in.
/// `optionalDependencies` wins over `dependencies` wins over
/// `devDependencies` when an alias is duplicated across groups
/// (first-write-wins over the dependency fields).
pub(super) fn manifest_alias_to_group(
    manifest: &PackageManifest,
) -> HashMap<String, DependencyGroup> {
    let mut out: HashMap<String, DependencyGroup> = HashMap::new();
    for group in [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev] {
        for (alias, _) in manifest.dependencies([group]) {
            out.entry(alias.to_string()).or_insert(group);
        }
    }
    out
}
/// Look up the user-written specifier for `alias` in the manifest's
/// `optionalDependencies` / `dependencies` / `devDependencies` maps —
/// plus `peerDependencies` when `auto_install_peers` materializes those
/// into the importer's dependencies. Returns `None` for an alias the
/// manifest doesn't declare in any of those groups, including a peer the
/// hoist installed while `autoInstallPeers` is off: such entries stay out
/// of the importer's `specifiers` map and are only reachable through the
/// snapshots graph.
pub(super) fn read_manifest_specifier(
    manifest: &PackageManifest,
    alias: &str,
    auto_install_peers: bool,
) -> Option<String> {
    let materialized_peers = auto_install_peers.then_some(DependencyGroup::Peer);
    for group in [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev]
        .into_iter()
        .chain(materialized_peers)
    {
        let group_key: &str = group.into();
        if let Some(map) = manifest.value().get(group_key).and_then(Value::as_object)
            && let Some(spec) = map.get(alias).and_then(Value::as_str)
        {
            return Some(spec.to_string());
        }
    }
    None
}
/// Build the version cell for an importer-level dependency.
pub(super) fn importer_dep_version(
    alias: &str,
    node: &DependenciesGraphNode,
) -> Result<ImporterDepVersion, ParseImporterDepVersionError> {
    let dep_path_str = node.dep_path.as_str();

    if let Some(target) = dep_path_str.strip_prefix("link:") {
        return Ok(ImporterDepVersion::Link(target.to_string()));
    }
    if let Some(target) = dep_path_str.strip_prefix("file:") {
        // An injected workspace dep reaches the `file:` arm (rather than
        // deduping back to `link:`) because its children weren't a subset
        // of the target project's direct deps, or `dedupeInjectedDeps` is off.
        return Ok(ImporterDepVersion::File(target.to_string()));
    }

    let real_name = real_name(&node.resolve_result);
    if let Some(real) = real_name.as_deref()
        && alias == real
        && let Some(rest) = dep_path_str.strip_prefix(real)
        && let Some(ver) = rest.strip_prefix('@')
        && let Ok(parsed) = ver.parse::<PkgVerPeer>()
    {
        return Ok(ImporterDepVersion::Regular(parsed));
    }
    let parsed = dep_path_str.parse::<ImporterDepVersion>()?;
    // An injected workspace dep reaches this point as its full peered
    // dep path, `<name>@file:<path>(peers)` — the bare `file:` strip
    // above only matches peerless dep paths.
    if let ImporterDepVersion::Alias(parsed_alias) = &parsed
        && let Some(ver) = self_aliased_file_ver(alias, parsed_alias)
    {
        let suffix = ver.to_string();
        let payload = suffix
            .strip_prefix("file:")
            .expect("a File version part always displays with the file: scheme");
        return Ok(ImporterDepVersion::File(payload.to_string()));
    }
    Ok(parsed)
}
/// `Some(version)` when `key` names a `file:` package aliased to its own
/// name. pnpm reserves the `<name>@<ref>` alias form for *renamed* deps
/// and writes the plain `file:<path>(peers)` ref when the alias equals
/// the package name; a self-aliased ref would double-prefix every
/// consumer that composes `alias@version` into a snapshot key (v11
/// readers, Bit's graph converter).
///
/// The dep path is the only place the name is available for these:
/// [`real_name`] is unset for directory resolutions, which learn their
/// name from the fetched manifest.
pub(super) fn self_aliased_file_ver<'a>(
    alias: &str,
    key: &'a PkgNameVerPeer,
) -> Option<&'a PkgVerPeer> {
    let aliased_to_own_name = match key.name.scope.as_deref() {
        Some(scope) => alias
            .strip_prefix('@')
            .and_then(|unscoped| unscoped.split_once('/'))
            .is_some_and(|(alias_scope, bare)| alias_scope == scope && bare == key.name.bare),
        None => alias == key.name.bare,
    };
    (aliased_to_own_name && matches!(key.suffix.version(), VersionPart::File(_)))
        .then_some(&key.suffix)
}
/// The previous importer's recorded entry for `name`, searched across
/// its `dependencies` / `optionalDependencies` / `devDependencies` maps
/// (mirrors the lookup order in
/// [`pnpm_resolving_deps_resolver`]'s `lockfile_reuse`). Used by the
/// pnpm/pnpm#10433 guard in [`build_importer`] to recover a workspace
/// dependency's prior `link:` entry.
pub(super) fn previous_importer_dep<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer
        .dependencies
        .as_ref()
        .and_then(|map| map.get(name))
        .or_else(|| importer.optional_dependencies.as_ref().and_then(|map| map.get(name)))
        .or_else(|| importer.dev_dependencies.as_ref().and_then(|map| map.get(name)))
}
/// The resolved package name for a graph node — the structured
/// `name_ver` when the resolver produced one, otherwise the `name` from
/// the fetched manifest (the case for a directory/workspace resolution,
/// whose `name_ver` is unset). Used to match a workspace dependency
/// against an `update <name>` scope in [`build_importer`].
pub(super) fn node_pkg_name(node: &DependenciesGraphNode) -> Option<String> {
    if let Some(name_ver) = node.resolve_result.name_ver.as_ref() {
        return Some(name_ver.name.to_string());
    }
    node.resolve_result.manifest.as_ref()?.get("name")?.as_str().map(str::to_string)
}
/// `Some(real_name)` when the resolver produced a structured name; `None`
/// for resolvers that learn the name from the fetched manifest (git,
/// tarball, file).
pub(super) fn real_name(result: &ResolveResult) -> Option<String> {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return Some(name_ver.name.to_string());
    }
    // `name_ver` is unset for resolutions that learn the canonical name
    // from the fetched manifest. Read it for the shapes whose `name@`
    // prefix is stripped off the importer entry:
    // - a remote (non-registry) http(s) tarball direct dep
    //   (`<name>@<tarball-url>` -> `version: <url>`), which a git-hosted
    //   dep's host archive URL also is,
    // - a runtime dep (`<name>@runtime:<ver>`, a Variations resolution ->
    //   `version: runtime:<ver>`),
    // - a git dep with no host archive (`<name>@git+<repo>#<commit>` ->
    //   `version: git+<repo>#<commit>`), the shape every non-host repo
    //   resolves to (ssh, self-hosted, `file:`).
    // `file:` resolutions stay on the `None` path: both callers strip
    // their `<name>@` prefix from the parsed dep path instead, via
    // [`self_aliased_file_ver`].
    let reads_name_from_manifest = match &result.resolution {
        LockfileResolution::Variations(_) | LockfileResolution::Git(_) => true,
        LockfileResolution::Tarball(tarball) => is_remote_http_tarball(&tarball.tarball),
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
pub(super) fn is_remote_http_tarball(tarball: &str) -> bool {
    tarball.starts_with("http:") || tarball.starts_with("https:")
}
