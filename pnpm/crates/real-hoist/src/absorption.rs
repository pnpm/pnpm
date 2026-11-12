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
    /// Hoisting or dedup-removing would change a resolved peer provider.
    /// The child stays under its parent to preserve the binding. Mirrors
    /// upstream's `getNodeHoistInfo` peer checks at
    /// [hoist.ts:414][peer-shadow-root] and
    /// [hoist.ts:454-479][peer-path], which upstream runs before its
    /// same-locator merge, so a copy whose peer comes from its parent
    /// is never merged into a root copy that resolves the peer
    /// elsewhere.
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
    let &HoistCtx { used, .. } = ctx;
    let mut decision = decision;

    // Used-dependency refusal: the root's subtree already
    // resolves this name from an ancestor directory; a different
    // version must not take the root's slot (and a same-version
    // dedup stays allowed — it resolves identically).
    if matches!(decision, AbsorbDecision::Free | AbsorbDecision::SameNode)
        && used
            .get(&child.name)
            .is_some_and(|provider| !same_ident(provider, child))
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
    // leaves the candidate in place. A dedup removes this
    // position's copy like a hoist does, so it is refused on
    // the same grounds: the root copy may share the locator
    // and still resolve its peer from a different provider.
    if matches!(decision, AbsorbDecision::Free | AbsorbDecision::SameNode)
        && would_shadow_peer(child, path, ctx, root_index)
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
    /// The child moved to the root. It is walked from there in the next
    /// wave of [`hoist_graph`](crate::hoist_graph), not now: upstream
    /// defers a hoisted node the same way, and the order decides which of
    /// two same-locator nodes claims the root slot.
    Hoisted,
    /// The child stays under this parent and is walked from here, with
    /// the path through the parent as its ancestor path.
    Stayed,
}

pub(super) fn apply_decision(
    decision: AbsorbDecision,
    child: &RcByPtr<HoisterResult>,
    node: &Rc<HoisterResult>,
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
) -> ChildStep {
    match decision {
        AbsorbDecision::Free => {
            node.dependencies.borrow_mut().shift_remove(child);
            node.hoisted_dependencies
                .borrow_mut()
                .insert(child.0.name.clone(), Rc::clone(&child.0));
            ctx.root.dependencies.borrow_mut().insert(child.clone());
            root_index.insert(child.0.name.clone(), child.clone());
            ChildStep::Hoisted
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
        // A later round may revisit the child with a different peer /
        // conflict / preference context; only `Border` is terminal, since
        // the limit boundary never moves.
        AbsorbDecision::Conflict
        | AbsorbDecision::PeerShadow
        | AbsorbDecision::PathShadow
        | AbsorbDecision::UsedShadow
        | AbsorbDecision::Border
        | AbsorbDecision::Defer => ChildStep::Stayed,
    }
}

/// Refuse a hoist or dedup that changes a resolved peer provider, or fills
/// a name the root receives as a peer. Snapshot edges take precedence
/// over ancestor lookup, including edges already hoisted out of a node.
/// A name the root lacks resolves through `ctx.used`, above the root.
fn would_shadow_peer(
    candidate: &HoisterResult,
    ancestor_path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
) -> bool {
    let &HoistCtx { root, used, .. } = ctx;
    // Root-shadow guard. Pacquet's wrapper builds the `.` root with
    // empty `peer_names` (it's a `Workspace`-kind node), so in
    // practice this check never fires today — kept for parity with
    // upstream and to stay correct if a future caller hands in a
    // root with declared peers.
    if root.peer_names.contains(&candidate.name) {
        return true;
    }

    let root_candidate = root_index.get(&candidate.name);
    candidate.peer_names
        .iter()
        .any(|peer_name| {
            let parent_provider = nearest_peer_provider(peer_name, path_below_root(ancestor_path));
            let Some(provider) = resolved_peer_provider(candidate, peer_name)
                .or_else(|| nearest_peer_provider(peer_name, ancestor_path))
            else {
                return false;
            };
            let target_provider = match root_candidate {
                Some(target) => resolved_peer_provider(&target.0, peer_name),
                // A hoist carries the nested provider along. With an ancestor
                // supplying the peer, the candidate stays near it, as upstream does.
                None if parent_provider.is_none() => nested_peer_provider(candidate, peer_name),
                None => None,
            }
            .or_else(|| root_index.get(peer_name).map(|target| Rc::clone(&target.0)))
            .or_else(|| used.get(peer_name).map(Rc::clone));
            !target_provider.is_some_and(|target| same_locator(&target, &provider))
        })
}

/// `ancestor_path` without its first entry, the hoisting root.
fn path_below_root(ancestor_path: &[Rc<HoisterResult>]) -> &[Rc<HoisterResult>] {
    ancestor_path
        .get(1..)
        .unwrap_or_default()
}

fn resolved_peer_provider(node: &HoisterResult, peer_name: &str) -> Option<Rc<HoisterResult>> {
    nested_peer_provider(node, peer_name)
        .or_else(|| {
            node.hoisted_dependencies
                .borrow()
                .get(peer_name)
                .map(Rc::clone)
        })
}

fn nested_peer_provider(node: &HoisterResult, peer_name: &str) -> Option<Rc<HoisterResult>> {
    node.dependencies
        .borrow()
        .iter()
        .find(|dependency| dependency.0.name == peer_name)
        .map(|dependency| Rc::clone(&dependency.0))
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
    ancestor_path
        .iter()
        .rev()
        .find_map(|ancestor| {
            ancestor.dependencies
                .borrow()
                .iter()
                .find(|dep| dep.0.name == *peer_name)
                .map(|dep| Rc::clone(&dep.0))
        })
}
