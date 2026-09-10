use super::{
    HashMap, HoistCtx, HoisterResult, Rc, RcByPtr, VecDeque, is_preferred_ident, path_shadowed,
    same_ident, same_locator,
};

/// Outcome of the per-child hoist decision at the root.
#[derive(Clone, Copy)]
pub(super) enum AbsorbDecision {
    /// Root's name slot is free; the child should be moved up to
    /// the root.
    Free,
    /// Root's name slot is free, but this candidate's ident is not
    /// the one currently preferred for its name (see
    /// [`build_hoist_ident_map`](crate::preferences::build_hoist_ident_map)). The candidate stays under its
    /// parent this pass; a later pass — after the preferred ident
    /// either claims the slot or is shifted out in
    /// [`hoist_into_root`](crate::hoist_into_root) — may reconsider it. Mirrors upstream's
    /// `hoistedIdent === node.ident` gate in `getNodeHoistInfo` at
    /// [hoist.ts:387][prefer-gate].
    ///
    /// [prefer-gate]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L387
    Defer,
    /// Root already holds a node with this locator (the same
    /// package was reachable through another parent path — possibly
    /// as a decoupled copy — and got hoisted earlier). The duplicate
    /// edge in the current parent just needs to be removed; the
    /// root's copy represents the subtree from here on. Mirrors
    /// upstream's same-locator merge in `hoistGraph` at
    /// [hoist.ts:521][merge].
    ///
    /// [merge]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L521
    SameNode,
    /// Root's name slot is taken by a different locator — a version
    /// conflict. The child stays under its current parent.
    Conflict,
    /// Hoisting would shadow a peer dependency one of the
    /// candidate's ancestors satisfies with a different ident than
    /// what the root provides. The child stays under its parent so
    /// the ancestor's peer resolution still finds the intended
    /// version. Mirrors upstream's `getNodeHoistInfo` peer checks
    /// at [hoist.ts:414][peer-shadow-root] and
    /// [hoist.ts:454-479][peer-path].
    ///
    /// [peer-shadow-root]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L414
    /// [peer-path]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L454-L479
    PeerShadow,
    /// An ancestor strictly below the root holds a *different*
    /// package version under the candidate's name. Hoisting (or
    /// dedup-removing) the candidate would make this subtree resolve
    /// the name to that nearer, conflicting copy instead, so the
    /// candidate stays nested. Ports the "filled by parent" scan in
    /// upstream's [`getNodeHoistInfo`][filled] (which vetoes
    /// `isNameAvailable` after the root-slot check).
    ///
    /// [filled]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L500-L516
    PathShadow,
    /// The subtree of the current hoist root resolves this name
    /// through an ancestor directory (a provider hoisted above the
    /// root earlier — see [`get_used_dependencies`](crate::get_used_dependencies)), and the
    /// candidate is a different version. Taking the root's name slot
    /// would shadow those resolutions, so the candidate stays
    /// nested. Ports the `usedDependencies` gate in upstream's
    /// [`getNodeHoistInfo`][used-gate].
    ///
    /// [used-gate]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L492-L497
    UsedShadow,
    /// The candidate sits beneath a hoisting border — its parent (or
    /// a higher ancestor) has a name listed in
    /// `opts.hoisting_limits` for the root locator. A bordered node's
    /// descendants stay nested beneath it rather than hoisting to the
    /// root, so the candidate stays under its parent. Mirrors
    /// upstream's `isHoistBorder` flag set during `cloneTree` from
    /// [`hoist.ts:707`][hoist-border], which blocks a bordered node's
    /// children from hoisting past it (not the bordered node itself).
    ///
    /// [hoist-border]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L707
    Border,
}

/// Where `child` belongs: the root's free / dedup / conflict verdict, with
/// the refusals that outrank a plain hoist layered on top.
pub(super) fn absorb_decision(
    child: &Rc<HoisterResult>,
    path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
    children_blocked: bool,
) -> AbsorbDecision {
    // A hoisting border on the parent (or any of its ancestors) keeps every
    // descendant nested, so the child stays put regardless of whether the
    // root slot is free. Decided before the free/dedup/conflict lookup
    // because the border wins outright.
    if children_blocked {
        return AbsorbDecision::Border;
    }
    let decision = root_slot_decision(child, root_index, ctx.hoist_ident_map);
    refuse_shadowed(decision, child, path, ctx, root_index)
}

fn root_slot_decision(
    child: &Rc<HoisterResult>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
    hoist_ident_map: &HashMap<String, VecDeque<String>>,
) -> AbsorbDecision {
    match root_index.get(&child.name) {
        None if is_preferred_ident(child, hoist_ident_map) => AbsorbDecision::Free,
        None => AbsorbDecision::Defer,
        Some(existing) if same_locator(&existing.0, child) => AbsorbDecision::SameNode,
        Some(_) => AbsorbDecision::Conflict,
    }
}

/// Downgrade a hoist or dedup that a copy shadowing `child` between its
/// position and the root would make wrong.
fn refuse_shadowed(
    decision: AbsorbDecision,
    child: &Rc<HoisterResult>,
    path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
) -> AbsorbDecision {
    let &HoistCtx { root, used, .. } = ctx;
    let mut decision = decision;

    // Used-dependency refusal: the root's subtree already
    // resolves this name from an ancestor directory; a different
    // version must not take the root's slot (and a same-version
    // dedup stays allowed — it resolves identically).
    if matches!(decision, AbsorbDecision::Free | AbsorbDecision::SameNode)
        && used.get(&child.name).is_some_and(|provider| !same_ident(provider, child))
    {
        decision = AbsorbDecision::UsedShadow;
    }

    // "Filled by parent" refusal: when a nearer ancestor's
    // node_modules already carries a different version of this
    // name, both hoisting and dedup-removal are wrong — either
    // way the requires at this position would start resolving to
    // the nearer, conflicting copy. Applies to `SameNode` too:
    // even if the root already holds this exact package, the
    // shadow between here and the root means this position needs
    // its own nested copy.
    if matches!(decision, AbsorbDecision::Free | AbsorbDecision::SameNode)
        && path_shadowed(child, path)
    {
        decision = AbsorbDecision::PathShadow;
    }

    // Peer-aware refusal layered on top of the basic
    // free / dedup / conflict decision. `Conflict` already
    // leaves the candidate in place and `SameNode` dedups
    // an already-hoisted package, so the peer check
    // only matters when we'd otherwise hoist.
    if matches!(decision, AbsorbDecision::Free) && would_shadow_peer(child, path, root, root_index)
    {
        decision = AbsorbDecision::PeerShadow;
    }

    decision
}

/// What is left to do with a child once its [`AbsorbDecision`] has been
/// applied to the graph.
pub(super) enum ChildStep {
    /// The edge was removed at this parent; there is nothing to descend into.
    Dropped,
    /// The child stays in the graph and must be walked. `path` is its
    /// ancestor path computed from its *new* position — the load-bearing
    /// detail: the recursion path always reflects the child's current
    /// position in the result graph, so peer checks deeper down see
    /// ancestors that are actually ancestors. `moved` reports whether
    /// applying the decision changed the graph.
    Descend { path: Vec<Rc<HoisterResult>>, moved: bool },
}

pub(super) fn apply_decision(
    decision: AbsorbDecision,
    child: &RcByPtr<HoisterResult>,
    node: &Rc<HoisterResult>,
    ctx: &HoistCtx<'_>,
    path_for_children: &[Rc<HoisterResult>],
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
) -> ChildStep {
    let root = ctx.root;
    // Root's direct children are already at root — no movement happens, and
    // their ancestor path is simply `[root]`.
    if Rc::ptr_eq(node, root) {
        return ChildStep::Descend { path: path_for_children.to_vec(), moved: false };
    }
    match decision {
        AbsorbDecision::Free => {
            node.dependencies.borrow_mut().shift_remove(child);
            node.hoisted_dependencies
                .borrow_mut()
                .insert(child.0.name.clone(), Rc::clone(&child.0));
            root.dependencies.borrow_mut().insert(child.clone());
            root_index.insert(child.0.name.clone(), child.clone());
            // Child is now a direct dep of root; its ancestor path collapses
            // to `[root]`.
            ChildStep::Descend { path: vec![Rc::clone(root)], moved: true }
        }
        AbsorbDecision::SameNode => {
            // A copy of this package is already at root;
            // strip the duplicate edge at this parent so the
            // deeper copy disappears. The root's copy
            // represents the subtree from here on (it is
            // walked as a root child every round), so there
            // is nothing to descend into.
            node.dependencies.borrow_mut().shift_remove(child);
            node.hoisted_dependencies
                .borrow_mut()
                .insert(child.0.name.clone(), Rc::clone(&child.0));
            ChildStep::Dropped
        }
        // Stays at the current parent, so the child's ancestor path is the
        // path through `node`. A later round may revisit it with a different
        // peer / conflict / preference context; only `Border` is terminal,
        // since the limit boundary never moves.
        AbsorbDecision::Conflict
        | AbsorbDecision::PeerShadow
        | AbsorbDecision::PathShadow
        | AbsorbDecision::UsedShadow
        | AbsorbDecision::Border
        | AbsorbDecision::Defer => {
            ChildStep::Descend { path: path_for_children.to_vec(), moved: false }
        }
    }
}

/// Return `true` when hoisting `candidate` onto the root would
/// shadow a peer dependency one of its ancestors already
/// satisfies with a different ident.
///
/// Implements two of the three peer guards upstream's
/// `getNodeHoistInfo` runs:
///
/// * **Root-shadow** — the candidate's own name appears in
///   `root.peer_names`. The root expects to *receive* this name as
///   a peer from its own parent, so promoting the candidate into
///   the root's name slot would change peer resolution for
///   anything that sees the root.
/// * **Ancestor-path mismatch** — for each peer name `P` in
///   `candidate.peer_names`, walk the candidate's ancestors from
///   deepest (immediate parent) toward the root. The first
///   ancestor that has a direct dep named `P` (and doesn't itself
///   peer-pass `P` through) is the one whose ident the candidate
///   resolves at runtime. If the root provides a *different*
///   ident for `P` (or none at all), promoting the candidate
///   would silently re-resolve its peer to the wrong package, so
///   we leave it nested.
///
/// The ancestor chain is per-path by construction: the DFS
/// decouples every node it descends into (see [`decouple_child`](crate::decouple_child)),
/// so a package shared by several parents gets a fresh peer ruling
/// on each path, matching upstream's per-path work tree.
fn would_shadow_peer(
    candidate: &HoisterResult,
    ancestor_path: &[Rc<HoisterResult>],
    root: &Rc<HoisterResult>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
) -> bool {
    // Root-shadow guard. Pacquet's wrapper builds the `.` root with
    // empty `peer_names` (it's a `Workspace`-kind node), so in
    // practice this check never fires today — kept for parity with
    // upstream and to stay correct if a future caller hands in a
    // root with declared peers.
    if root.peer_names.contains(&candidate.name) {
        return true;
    }

    candidate.peer_names.iter().any(|peer_name| {
        // No ancestor (excluding root) providing the peer means the candidate
        // either resolves it at root or leaves it unsatisfied. Either case is
        // "no shadow".
        let Some(provider) = nearest_peer_provider(peer_name, ancestor_path) else {
            return false;
        };
        // Compare the provider's locator (identity is too strict — decoupled
        // copies of one package are distinct allocations) against root's
        // current slot for the same name. Root carrying this exact provider
        // means promoting the candidate doesn't change resolution; a
        // different ident, or no entry at all, means hoisting would shadow.
        !root_index.get(peer_name).is_some_and(|at_root| same_locator(&at_root.0, &provider))
    })
}

/// The deepest ancestor in `ancestor_path` that carries `peer_name` as a
/// direct dependency, if any.
///
/// The walk goes deepest-first so the closest provider wins. An ancestor
/// that merely passes the peer through — its own `peer_names` includes the
/// name and it has no direct dep for it — is walked past, since the actual
/// provider is then a parent of that ancestor.
fn nearest_peer_provider(
    peer_name: &str,
    ancestor_path: &[Rc<HoisterResult>],
) -> Option<Rc<HoisterResult>> {
    ancestor_path.iter().rev().find_map(|ancestor| {
        ancestor
            .dependencies
            .borrow()
            .iter()
            .find(|dep| dep.0.name == *peer_name)
            .map(|dep| Rc::clone(&dep.0))
    })
}
