use super::{
    HashMap,
    HoistError,
    HoistOpts,
    HoisterDependencyKind,
    HoisterTree,
    IndexMap,
    IndexSet,
    Lockfile,
    PkgName,
    PkgNameVerPeer,
    ProjectSnapshot,
    Rc,
    RcByPtr,
    RefCell,
    SnapshotEntry,
    VersionPart,
};
use std::fmt::Write as _;

pub(crate) mod peers;

pub(super) fn external_placeholder(dep: &str) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        identity: None,
        name: dep.to_string(),
        ident_name: dep.to_string(),
        reference: "link:".to_string(),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::ExternalSoftLink,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    })
}

/// A `link:<root>/...` dependency points inside the package that declares it,
/// so it must stay next to that package. The hoister never moves a
/// `Workspace` node, and keeping the node reserves its name there.
fn package_root_link_node(
    alias: &PkgName,
    dep_ref: &pnpm_lockfile::SnapshotDepRef,
) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        identity: None,
        name: alias.to_string(),
        ident_name: alias.to_string(),
        reference: dep_ref.to_string(),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::Workspace,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    })
}

/// `HashMap` iteration order is non-deterministic; sort so the
/// output tree is stable across runs (matters for snapshot
/// tests).
/// Empty every node's children. Dependency cycles in the lockfile make
/// the tree a web of reference cycles that would otherwise never be freed.
pub(super) fn tear_down(root: &Rc<HoisterTree>) {
    let mut pending = vec![Rc::clone(root)];
    while let Some(node) = pending.pop() {
        let children = std::mem::take(&mut *node.dependencies.borrow_mut());
        pending.extend(children.into_iter().map(|child| child.0));
    }
}

pub(super) fn sorted_non_root_importers(lockfile: &Lockfile) -> Vec<(&String, &ProjectSnapshot)> {
    let mut non_root: Vec<(&String, &ProjectSnapshot)> = lockfile.importers
        .iter()
        .filter(|(id, _)| id.as_str() != Lockfile::ROOT_IMPORTER_KEY)
        .collect();
    non_root.sort_by(|a, b| a.0.cmp(b.0));
    non_root
}

pub(super) fn importer_node(
    importer_id: &str,
    children: IndexSet<RcByPtr<HoisterTree>>,
) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        identity: None,
        name: percent_encode_path(importer_id),
        ident_name: percent_encode_path(importer_id),
        reference: format!("workspace:{importer_id}"),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::Workspace,
        hoist_priority: 0,
        dependencies: RefCell::new(children),
    })
}

/// Conversion-phase cache: one [`HoisterTree`] per `(alias, snapshot
/// key)` edge target. Every node carries its own snapshot key as its
/// `reference`; the hoister collapses the peer variants of one package
/// version into one identity itself (see [`pkg_id`]).
#[derive(Default)]
pub(super) struct TreeCache {
    pub(super) nodes: HashMap<String, Rc<HoisterTree>>,
    /// See [`crate::repair`].
    pub(super) regular_peers: crate::repair::RegularPeers,
}

/// Dependency groups merged as pnpm v11 spreads them into one object:
/// group by group, each in the alias order the lockfile writes, and a
/// later group overwrites an alias without moving it.
fn spread_groups<'map, Value>(
    groups: &[(&'map Option<HashMap<PkgName, Value>>, bool)],
) -> IndexMap<&'map PkgName, (&'map Value, bool)> {
    let mut merged = IndexMap::new();
    for (deps, optional) in groups {
        let mut entries: Vec<(&PkgName, &Value)> = deps.iter().flatten().collect();
        entries.sort_by_key(|(alias, _)| alias.to_string());
        for (alias, value) in entries {
            merged.insert(alias, (value, *optional));
        }
    }
    merged
}

pub(super) fn collect_importer_deps(
    importer: &ProjectSnapshot,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    cache: &mut TreeCache,
    out: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    // Merge `dependencies + devDependencies + optionalDependencies`
    // into one alias-keyed object; on a duplicate alias the last
    // write wins. `ResolvedDependencyMap` is a HashMap so declaration
    // order is lost; merge into a `HashMap` (last write wins) and emit
    // in alias-sorted order so the build is deterministic regardless
    // of map seed.
    let groups = [
        (&importer.dependencies, false),
        (&importer.dev_dependencies, false),
        (&importer.optional_dependencies, true),
    ];
    for (alias, (spec, optional)) in spread_groups(&groups) {
        // For an aliased importer dep (`ImporterDepVersion::Alias`),
        // the snapshot key is the alias's own (name, suffix);
        // [`ImporterDepVersion::resolved_key`] returns that.
        // Transitive npm-aliases (modelled via `SnapshotDepRef::Alias`)
        // are handled in `collect_snapshot_deps`.
        //
        // `link:` deps (cross-importer `workspace:*` resolutions, see
        // [`ImporterDepVersion::Link`]) don't live in the virtual
        // store, they're directory symlinks materialised by
        // [`pnpm_package_manager::SymlinkDirectDependencies`],
        // so they have no snapshot to hoist and we skip them here.
        let Some(dep_key) = spec.version.resolved_key(alias) else {
            continue;
        };
        let Some(node) = build_dep_node(alias, &dep_key, optional, lockfile, opts, cache)? else {
            continue;
        };
        out.insert(RcByPtr(node));
    }
    Ok(())
}

/// Returns `Ok(None)` for an optional edge whose target snapshot is
/// missing, a skipped optional dependency (platform mismatch, fetch
/// failure) is filtered out of the current lockfile, and pnpm skips
/// such an edge rather than treating the lockfile as broken. A
/// missing snapshot behind a non-optional edge is still
/// [`HoistError::LockfileMissingDependency`].
fn build_dep_node(
    alias: &PkgName,
    dep_key: &PkgNameVerPeer,
    optional: bool,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    cache: &mut TreeCache,
) -> Result<Option<Rc<HoisterTree>>, HoistError> {
    // Cache key is `<alias>:<dep_key>`, two different aliases
    // pointing at the same package are intentionally different nodes
    // (the node's `name` field differs), so they shouldn't share a
    // cache slot.
    let cache_key = format!("{alias}:{dep_key}");
    if let Some(existing) = cache.nodes.get(&cache_key) {
        return Ok(Some(Rc::clone(existing)));
    }

    let Some(snapshot) = lookup_snapshot(dep_key, optional, lockfile)? else {
        return Ok(None);
    };

    // Construct the node with an empty `dependencies` cell, stash
    // it in the cache, then recurse and populate the cell in place.
    // A back-edge that hits the same `cache_key` during the
    // recursion gets the same `Rc<HoisterTree>`, by the time the
    // outer call returns the cell holds the populated set, and the
    // shared-by-identity invariant the hoister algorithm relies on
    // survives.
    let peers =
        peers::peer_view(dep_key, snapshot, lockfile, opts, cache.regular_peers.get(dep_key));
    let node = Rc::new(HoisterTree {
        identity: peers.identity,
        name: alias.to_string(),
        ident_name: dep_key.name.to_string(),
        reference: dep_key.to_string(),
        peer_names: peers.names,
        dependency_kind: HoisterDependencyKind::Regular,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    });
    cache.nodes.insert(cache_key, Rc::clone(&node));

    let mut children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
    collect_snapshot_deps(snapshot, lockfile, opts, cache, &mut children)?;
    let context = (lockfile, opts);
    link_transitive_peer_providers(dep_key, snapshot, context, cache, &mut children)?;
    *node.dependencies.borrow_mut() = children;
    Ok(Some(node))
}

/// Give each transitive peer an edge to the provider the snapshot key's
/// peer suffix records, as a Yarn virtual package links its peers.
///
/// A lockfile lists a transitive peer only by name. Without the edge the
/// hoister's peer check stops at this node and calls the peer satisfied,
/// so a descendant could be deduplicated into a copy that resolves the
/// peer to another version. A peer edge is never hoisted and never
/// emitted, so the edge adds no directory.
fn link_transitive_peer_providers(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    (lockfile, opts): (&Lockfile, &HoistOpts),
    cache: &mut TreeCache,
    children: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    let has_edge = |name: &str| {
        children
            .iter()
            .any(|child| child.0.name == name)
    };
    let providers = peers::unlinked_peer_providers(dep_key, snapshot, lockfile, has_edge);
    for provider in providers {
        if let Some(provider_node) =
            build_dep_node(&provider.name, &provider, true, lockfile, opts, cache)?
        {
            children.insert(RcByPtr(provider_node));
        }
    }
    Ok(())
}

/// `dep_key`'s entry in the `snapshots:` map. A missing entry is an error
/// unless the dependency is optional, in which case there is nothing
/// to hoist.
fn lookup_snapshot<'a>(
    dep_key: &PkgNameVerPeer,
    optional: bool,
    lockfile: &'a Lockfile,
) -> Result<Option<&'a SnapshotEntry>, HoistError> {
    match lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(dep_key))
    {
        Some(snapshot) => Ok(Some(snapshot)),
        None if optional => Ok(None),
        None => Err(HoistError::LockfileMissingDependency { pkg_key: dep_key.to_string() }),
    }
}

fn collect_snapshot_deps(
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    cache: &mut TreeCache,
    out: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    let groups = [(&snapshot.dependencies, false), (&snapshot.optional_dependencies, true)];
    for (alias, (dep_ref, optional)) in spread_groups(&groups) {
        // `dep_ref.resolve(alias)` returns the *snapshot lookup
        // key*: `<alias>@<ver>` for `Plain`, `<target>@<ver>` for
        // an npm-alias `Alias`. Pass that as `dep_key` so the
        // snapshot lookup hits the right entry. The node's exposed
        // `name` stays `alias`; only the lookup uses the resolved
        // target name.
        //
        // `link:` deps return `None`; they have no snapshot to
        // hoist (the install layer materialises them as direct
        // directory symlinks), so we skip them here.
        let Some(dep_key) = dep_ref.resolve(alias) else {
            if dep_ref.package_root_link_target().is_some() {
                out.insert(RcByPtr(package_root_link_node(alias, dep_ref)));
            }
            continue;
        };
        let Some(node) = build_dep_node(alias, &dep_key, optional, lockfile, opts, cache)? else {
            continue;
        };
        out.insert(RcByPtr(node));
    }
    Ok(())
}

/// The identity every peer variant of one package version collapses
/// onto: the snapshot key with its peer suffix (and the patch hash
/// that sits alongside it) removed.
///
/// The hoister maps the id to the first snapshot key it sees for it
/// and stamps that key on every variant, so only that first key
/// survives as their shared `reference`. Anything indexing the hoist
/// result by package therefore has to key *and* look up by this id
/// rather than by the snapshot key an edge declares; otherwise every
/// edge on a collapsed variant finds nothing and drops out of the
/// layout.
///
/// One id can still cover several directories: [`HoisterTree`] nodes
/// are interned per `(alias, snapshot key)`, so an alias exposing a
/// package under a second name gets a node, and a directory, of its
/// own. An index over the result holds the list and resolves an edge
/// to its first entry.
///
/// An injected directory dependency (a `file:` version that is not a
/// local tarball) keeps its peer suffix: every variant of it is a
/// separate on-disk copy of the local package, materialized with its
/// own peer-resolved dependency set, so collapsing the variants would
/// rewire every dependent of the losing one onto the survivor's
/// children (Bit's root components pin conflicting peers across such
/// copies on purpose). The registry collapse exists to stop
/// peer-variant explosion on large lockfiles; directory snapshots are
/// one per injected workspace package and cannot explode that way. A
/// local tarball (`file:foo.tgz`) unpacks the same archive for every
/// variant like a registry package, so it collapses like one, the
/// boundary is [`pnpm_local_spec::is_tarball_filename`], the same one
/// the lockfile itself draws for `file:` resolutions.
#[must_use]
pub fn pkg_id(dep_key: &PkgNameVerPeer) -> String {
    if let VersionPart::File(path) = dep_key.suffix.version()
        && !pnpm_local_spec::is_tarball_filename(path)
    {
        return dep_key.to_string();
    }
    dep_key.without_peer().to_string()
}

/// Encode an importer id for use as a child node's `name` (and in
/// the hoisting-limits locator keys built by
/// `pnpm_package_manager::get_hoisting_limits`). Matches
/// `encodeURIComponent`: percent-encode everything except
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Pacquet workspace importers are
/// filesystem-relative paths, so the common case is alphanumeric +
/// `/` + `-` + `_`. Encode `/` (since it would confuse
/// `node_modules` directory parsing) and pass the rest through; if a
/// richer set ever shows up the function can switch to a full
/// encoder without touching call sites.
#[must_use]
pub fn percent_encode_path(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '-'
            | '_'
            | '.'
            | '!'
            | '~'
            | '*'
            | '\''
            | '('
            | ')' => out.push(ch),
            '/' => out.push_str("%2F"),
            other => {
                // Best-effort %xx encode for the ASCII subset we
                // expect in importer ids. Anything else is left
                // verbatim, pacquet's lockfile doesn't currently
                // hand the wrapper non-ASCII paths.
                if (other as u32) < 0x80 {
                    write!(out, "%{:02X}", other as u32).unwrap();
                } else {
                    out.push(other);
                }
            }
        }
    }
    out
}
