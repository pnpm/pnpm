use super::{
    BTreeSet, Cell, HashMap, HoistError, HoistOpts, HoisterDependencyKind, HoisterResult,
    HoisterTree, IndexSet, Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, Rc, RcByPtr,
    RefCell, SnapshotEntry, VersionPart,
};
use std::fmt::Write as _;

pub(super) fn external_placeholder(dep: &str) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        name: dep.to_string(),
        ident_name: dep.to_string(),
        reference: "link:".to_string(),
        peer_names: BTreeSet::new(),
        dependency_kind: HoisterDependencyKind::ExternalSoftLink,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    })
}

/// `HashMap` iteration order is non-deterministic; sort so the
/// output tree is stable across runs (matters for snapshot
/// tests).
pub(super) fn sorted_non_root_importers(lockfile: &Lockfile) -> Vec<(&String, &ProjectSnapshot)> {
    let mut non_root: Vec<(&String, &ProjectSnapshot)> = lockfile
        .importers
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
        name: percent_encode_path(importer_id),
        ident_name: percent_encode_path(importer_id),
        reference: format!("workspace:{importer_id}"),
        peer_names: BTreeSet::new(),
        dependency_kind: HoisterDependencyKind::Workspace,
        hoist_priority: 0,
        dependencies: RefCell::new(children),
    })
}

/// Conversion-phase caches. `nodes` interns one [`HoisterTree`] per
/// `(alias, snapshot key)` edge target. `dep_key_by_pkg_id` maps a
/// package id (see [`pkg_id`]) to the first snapshot key seen for
/// it: every peer-suffix variant of one package version
/// gets that first key as its `reference`, so the hoister sees one
/// locator per version and dedups the variants instead of
/// conflict-nesting a copy of each. Ports `depPathByPkgId` from pnpm
/// v11's `@pnpm/real-hoist` wrapper — without the collapse, a
/// peer-variant-heavy lockfile turns the per-path hoist walk into a
/// combinatorial explosion of nested conflict copies.
#[derive(Default)]
pub(super) struct TreeCache {
    pub(super) nodes: HashMap<String, Rc<HoisterTree>>,
    pub(super) dep_key_by_pkg_id: HashMap<String, PkgNameVerPeer>,
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
    let mut merged: HashMap<&PkgName, (&pnpm_lockfile::ResolvedDependencySpec, bool)> =
        HashMap::new();
    for (deps, optional) in [
        (&importer.dependencies, false),
        (&importer.dev_dependencies, false),
        (&importer.optional_dependencies, true),
    ] {
        for (alias, spec) in deps.iter().flatten() {
            merged.insert(alias, (spec, optional));
        }
    }
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by_key(|(alias, _)| alias.to_string());
    for (alias, (spec, optional)) in entries {
        // For an aliased importer dep (`ImporterDepVersion::Alias`),
        // the snapshot key is the alias's own (name, suffix);
        // [`ImporterDepVersion::resolved_key`] returns that.
        // Transitive npm-aliases (modelled via `SnapshotDepRef::Alias`)
        // are handled in `collect_snapshot_deps`.
        //
        // `link:` deps (cross-importer `workspace:*` resolutions, see
        // [`ImporterDepVersion::Link`]) don't live in the virtual
        // store — they're directory symlinks materialised by
        // [`pnpm_package_manager::SymlinkDirectDependencies`] —
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
/// missing — a skipped optional dependency (platform mismatch, fetch
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
    // Cache key is `<alias>:<dep_key>` — two different aliases
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
    // recursion gets the same `Rc<HoisterTree>` — by the time the
    // outer call returns the cell holds the populated set, and the
    // shared-by-identity invariant the hoister algorithm relies on
    // survives.
    // The reference is the *canonical* snapshot key for this package
    // version — the first-seen peer-suffix variant — not necessarily
    // `dep_key` itself (see [`TreeCache::dep_key_by_pkg_id`]). The
    // node's own children still come from `dep_key`'s snapshot,
    // matching the TS wrapper, which reads `pkgSnapshot` from the
    // original depPath while stamping `depPathByPkgId.get(id)` as the
    // reference.
    let reference = cache
        .dep_key_by_pkg_id
        .entry(pkg_id(dep_key))
        .or_insert_with(|| dep_key.clone())
        .to_string();
    let node = Rc::new(HoisterTree {
        name: alias.to_string(),
        ident_name: dep_key.name.to_string(),
        reference,
        peer_names: peer_names_of(dep_key, snapshot, lockfile, opts),
        dependency_kind: HoisterDependencyKind::Regular,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    });
    cache.nodes.insert(cache_key, Rc::clone(&node));

    let mut children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
    collect_snapshot_deps(snapshot, lockfile, opts, cache, &mut children)?;
    *node.dependencies.borrow_mut() = children;
    Ok(Some(node))
}

/// `dep_key`'s entry in the `snapshots:` map. A missing entry is an error
/// unless the dependency is optional, in which case there is simply nothing
/// to hoist.
fn lookup_snapshot<'a>(
    dep_key: &PkgNameVerPeer,
    optional: bool,
    lockfile: &'a Lockfile,
) -> Result<Option<&'a SnapshotEntry>, HoistError> {
    match lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(dep_key)) {
        Some(snapshot) => Ok(Some(snapshot)),
        None if optional => Ok(None),
        None => Err(HoistError::LockfileMissingDependency { pkg_key: dep_key.to_string() }),
    }
}

/// The node's peer-name set: `peerDependencies` (from the `packages:` map)
/// plus `transitivePeerDependencies` (from the `snapshots:` map). Empty when
/// `auto_install_peers` is on, so the hoister moves freely.
fn peer_names_of(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    opts: &HoistOpts,
) -> BTreeSet<String> {
    if opts.auto_install_peers {
        return BTreeSet::new();
    }
    let mut peer_names = declared_peer_names(dep_key, lockfile);
    peer_names.extend(snapshot.transitive_peer_dependencies.iter().flatten().cloned());
    peer_names
}

fn declared_peer_names(dep_key: &PkgNameVerPeer, lockfile: &Lockfile) -> BTreeSet<String> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return BTreeSet::new();
    };
    let Some(meta) = packages.get(&dep_key.without_peer()) else {
        return BTreeSet::new();
    };
    meta.peer_dependencies.iter().flatten().map(|(name, _)| name.clone()).collect()
}

fn collect_snapshot_deps(
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    cache: &mut TreeCache,
    out: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    let mut merged: HashMap<&PkgName, (&pnpm_lockfile::SnapshotDepRef, bool)> = HashMap::new();
    for (deps, optional) in
        [(&snapshot.dependencies, false), (&snapshot.optional_dependencies, true)]
    {
        for (alias, dep_ref) in deps.iter().flatten() {
            merged.insert(alias, (dep_ref, optional));
        }
    }
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by_key(|(alias, _)| alias.to_string());
    for (alias, (dep_ref, optional)) in entries {
        // `dep_ref.resolve(alias)` returns the *snapshot lookup
        // key*: `<alias>@<ver>` for `Plain`, `<target>@<ver>` for
        // an npm-alias `Alias`. Pass that as `dep_key` so the
        // snapshot lookup hits the right entry. The node's exposed
        // `name` stays `alias`; only the lookup uses the resolved
        // target name.
        //
        // `link:` deps return `None` — they have no snapshot to
        // hoist (the install layer materialises them as direct
        // directory symlinks), so we skip them here.
        let Some(dep_key) = dep_ref.resolve(alias) else {
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
/// onto: the snapshot key with its peer suffix — and the patch hash
/// that sits alongside it — removed.
///
/// The hoister maps the id to the first snapshot key it sees for it
/// and stamps that key on every variant, so only that first key
/// survives as their shared `reference`. Anything indexing the hoist
/// result by package therefore has to key *and* look up by this id
/// rather than by the snapshot key an edge declares — otherwise every
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
/// variant like a registry package, so it collapses like one — the
/// boundary is [`pnpm_lockfile::is_local_tarball_path`], the same one
/// the lockfile itself draws for `file:` resolutions.
#[must_use]
pub fn pkg_id(dep_key: &PkgNameVerPeer) -> String {
    if let VersionPart::File(path) = dep_key.suffix.version()
        && !pnpm_lockfile::is_local_tarball_path(path)
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
                // verbatim — pacquet's lockfile doesn't currently
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

#[derive(Default)]
pub(super) struct ConvertContext {
    pub(super) result_by_tree: HashMap<*const HoisterTree, Rc<HoisterResult>>,
}

pub(super) fn convert(tree: &HoisterTree, context: &mut ConvertContext) -> Rc<HoisterResult> {
    let ptr = std::ptr::from_ref::<HoisterTree>(tree);
    if let Some(existing) = context.result_by_tree.get(&ptr) {
        return Rc::clone(existing);
    }
    // Stash a node with empty `dependencies`, then recurse and
    // populate the cell in place. Anyone reached via a back-edge
    // gets `Rc::clone` of the same allocation and reads the
    // (eventually-populated) cell — matches the in-place mutation
    // semantics the real hoist algorithm needs.
    let mut refs = BTreeSet::new();
    refs.insert(tree.reference.clone());
    let node = Rc::new(HoisterResult {
        name: tree.name.clone(),
        ident_name: tree.ident_name.clone(),
        references: RefCell::new(refs),
        peer_names: tree.peer_names.clone(),
        dependencies: RefCell::new(IndexSet::new()),
        hoisted_dependencies: RefCell::new(HashMap::new()),
        decoupled: Cell::new(false),
    });
    context.result_by_tree.insert(ptr, Rc::clone(&node));

    // Collect the children before recursing so we can drop the
    // `Ref<'_, IndexSet<...>>` borrow on `tree.dependencies`. The
    // recursion only reads (not mutates) `HoisterTree` cells, so
    // holding the borrow across recursive calls is technically
    // safe, but releasing it keeps the panic surface smaller if
    // the algorithm later grows a mutation pass over the input.
    let to_convert: Vec<RcByPtr<HoisterTree>> =
        tree.dependencies.borrow().iter().cloned().collect();
    let mut children: IndexSet<RcByPtr<HoisterResult>> = IndexSet::new();
    for child in to_convert {
        children.insert(RcByPtr(convert(&child.0, context)));
    }
    *node.dependencies.borrow_mut() = children;
    node
}
