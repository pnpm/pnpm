use super::{
    HashMap, HashSet, HoistCtx, HoisterResult, Rc, RcByPtr, VecDeque, is_preferred_ident,
    path_shadowed, same_locator,
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
    /// An ancestor strictly below the root holds a different locator,
    /// a peer variant included, under the candidate's name. Hoisting (or
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
    /// candidate is a different locator, a peer variant included.
    /// Taking the root's name slot
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
    // locator, a peer variant included, must not take the root's slot.
    if matches!(decision, AbsorbDecision::Free | AbsorbDecision::SameNode)
        && used
            .get(&child.name)
            .is_some_and(|provider| !same_locator(provider, child))
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
    let &HoistCtx { root, .. } = ctx;
    // Root-shadow guard. Pacquet's wrapper builds the `.` root with
    // empty `peer_names` (it's a `Workspace`-kind node), so in
    // practice this check never fires today — kept for parity with
    // upstream and to stay correct if a future caller hands in a
    // root with declared peers.
    if root.peer_names.contains(&candidate.name) {
        return true;
    }

    candidate.peer_names
        .iter()
        .any(|peer_name| {
            let Some(providers) = resolved_peer_providers(candidate, peer_name, || {
                nearest_peer_provider(peer_name, ancestor_path)
            }) else {
                return false;
            };
            let targets =
                target_peer_providers(candidate, peer_name, ancestor_path, ctx, root_index);
            !targets.is_some_and(|targets| same_peer_providers(&providers, &targets))
        })
}

fn same_peer_providers(left: &[Rc<HoisterResult>], right: &[Rc<HoisterResult>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|provider| right.iter().any(|target| same_locator(target, provider)))
}

fn target_peer_providers(
    candidate: &HoisterResult,
    peer_name: &str,
    ancestor_path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
) -> Option<Vec<Rc<HoisterResult>>> {
    let inherited_provider = || {
        root_index
            .get(peer_name)
            .map(|target| Rc::clone(&target.0))
            .or_else(|| ctx.used.get(peer_name).map(Rc::clone))
    };
    match root_index.get(&candidate.name) {
        Some(target) => resolved_peer_providers(&target.0, peer_name, inherited_provider),
        // A hoist carries the nested provider along. With an ancestor
        // supplying the peer, the candidate stays near it, as upstream does.
        None if nearest_peer_provider(peer_name, path_below_root(ancestor_path)).is_none() => {
            resolved_peer_providers(candidate, peer_name, inherited_provider)
        }
        None => inherited_provider().map(|provider| vec![provider]),
    }
}

/// `ancestor_path` without its first entry, the hoisting root.
fn path_below_root(ancestor_path: &[Rc<HoisterResult>]) -> &[Rc<HoisterResult>] {
    ancestor_path
        .get(1..)
        .unwrap_or_default()
}

fn direct_peer_provider(node: &HoisterResult, peer_name: &str) -> Option<Rc<HoisterResult>> {
    nested_peer_provider(node, peer_name)
        .or_else(|| {
            node.hoisted_dependencies
                .borrow()
                .get(peer_name)
                .map(Rc::clone)
        })
}

fn resolved_peer_providers(
    node: &HoisterResult,
    peer_name: &str,
    inherited_provider: impl FnOnce() -> Option<Rc<HoisterResult>>,
) -> Option<Vec<Rc<HoisterResult>>> {
    if let Some(provider) = direct_peer_provider(node, peer_name) {
        return Some(vec![provider]);
    }
    let mut providers = Vec::new();
    let inherits =
        collect_transitive_peer_providers(node, peer_name, &mut HashSet::new(), &mut providers);
    if (inherits || providers.is_empty())
        && let Some(provider) = inherited_provider()
    {
        add_peer_provider(&mut providers, provider);
    }
    (!providers.is_empty()).then_some(providers)
}

/// Transitive peers belong to descendant snapshots, whose recorded edges
/// remain authoritative even after their dependencies have hoisted away.
/// Returns whether a branch needs a provider from the candidate's ancestors.
fn collect_transitive_peer_providers(
    node: &HoisterResult,
    peer_name: &str,
    visited: &mut HashSet<*const HoisterResult>,
    providers: &mut Vec<Rc<HoisterResult>>,
) -> bool {
    if !visited.insert(node) {
        return false;
    }
    if let Some(provider) = direct_peer_provider(node, peer_name) {
        add_peer_provider(providers, provider);
        return false;
    }
    let dependencies = node.dependencies.borrow();
    let hoisted = node.hoisted_dependencies.borrow();
    let mut children = dependencies
        .iter()
        .map(|child| &child.0)
        .chain(hoisted.values())
        .filter(|child| child.peer_names.contains(peer_name))
        .peekable();
    let mut inherits = children.peek().is_none();
    for child in children {
        inherits |= collect_transitive_peer_providers(child, peer_name, visited, providers);
    }
    inherits
}

fn add_peer_provider(providers: &mut Vec<Rc<HoisterResult>>, provider: Rc<HoisterResult>) {
    if !providers.iter().any(|existing| same_locator(existing, &provider)) {
        providers.push(provider);
    }
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
