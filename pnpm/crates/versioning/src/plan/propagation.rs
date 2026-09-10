use super::{
    AssembleContext, BTreeMap, BTreeSet, BumpState, ChangeIntent, DependencyUpdate, HashMap,
    HashSet, IntentBumpType, InternalDep, PackageConsumption, Participant, PlannedRelease,
    ReleaseBumpType, ReleaseCause, VersioningError, apply_epic_band_versions,
    apply_fixed_group_versions, compute_new_version, epic_rebase_floor,
    materialize_workspace_range, max_bump_type, max_bump_type_of, range_accepts,
};

/// The intents bearing on each participant: those still pending, and those
/// a prerelease lane already consumed.
pub(super) struct PlanIntents<'i> {
    pub(super) pending_by_dir: BTreeMap<String, Vec<&'i ChangeIntent>>,
    pub(super) lane_consumed_by_dir: BTreeMap<String, Vec<&'i ChangeIntent>>,
}

/// Seed the state with the bump each participant's own intents ask for.
pub(super) fn seed_bumps(
    ctx: &AssembleContext<'_>,
    intents: &PlanIntents<'_>,
    selection: Option<&HashSet<String>>,
    state: &mut BTreeMap<String, BumpState>,
) {
    let selected = |dir: &String| selection.is_none_or(|selected| selected.contains(dir));

    for (dir, pending) in intents.pending_by_dir.iter().filter(|(dir, _)| selected(dir)) {
        if let Some(direct) =
            max_bump_type(pending.iter().filter_map(|intent| ctx.intent_bump_for(intent, dir)))
        {
            bump_at_least(state, dir, direct, ReleaseCause::Intent);
        }
    }

    // A package that left its lane releases the accumulated stable version
    // even when no new intents are pending.
    let graduated = intents
        .lane_consumed_by_dir
        .iter()
        .filter(|(dir, _)| selected(dir) && !ctx.lanes_by_dir.contains_key(*dir));
    for (dir, lane_consumed) in graduated {
        if let Some(bump) = max_bump_type(
            lane_consumed.iter().filter_map(|intent| ctx.intent_bump_for(intent, dir)),
        ) {
            bump_at_least(state, dir, bump, ReleaseCause::Intent);
        }
    }
}

pub(super) fn compute_versions(
    ctx: &AssembleContext<'_>,
    intents: &PlanIntents<'_>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
) {
    let cumulative =
        |dir: &str, planned: ReleaseBumpType| cumulative_bump(ctx, intents, dir, planned);
    new_versions.clear();
    for (dir, pkg_state) in state {
        let participant = &ctx.participants[dir.as_str()];
        new_versions.insert(
            dir.clone(),
            compute_new_version(
                participant.current_version,
                pkg_state.bump_type,
                ctx.lanes_by_dir.get(dir).map(String::as_str),
                cumulative(dir, pkg_state.bump_type),
                ctx.opts.unpublished_dirs.contains(dir),
            ),
        );
    }
    apply_fixed_group_versions(
        ctx.participants,
        state,
        new_versions,
        &cumulative,
        ctx.fixed_groups,
        ctx.lanes_by_dir,
    );
    apply_epic_band_versions(ctx.participants, state, new_versions, ctx.epics, ctx.lanes_by_dir);
}

/// The bump a graduating package's version has to clear: the planned bump,
/// widened by every bump its lane already consumed.
pub(super) fn cumulative_bump(
    ctx: &AssembleContext<'_>,
    intents: &PlanIntents<'_>,
    dir: &str,
    planned: ReleaseBumpType,
) -> ReleaseBumpType {
    intents
        .lane_consumed_by_dir
        .get(dir)
        .into_iter()
        .flatten()
        .filter_map(|intent| ctx.intent_bump_for(intent, dir))
        .filter_map(IntentBumpType::release)
        .chain([planned])
        .max()
        .unwrap_or(planned)
}

/// One round of the fixpoint: widen the planned bumps until dependents,
/// fixed groups and epic bands all agree. Reports whether anything changed.
pub(super) fn propagate_bumps(
    ctx: &AssembleContext<'_>,
    new_versions: &BTreeMap<String, String>,
    state: &mut BTreeMap<String, BumpState>,
) -> bool {
    let mut changed = false;
    for (dependent_dir, target_name, target_new_version) in
        forced_dependency_bumps(ctx.participants, new_versions)
    {
        changed |=
            bump_at_least(state, dependent_dir, ReleaseBumpType::Patch, ReleaseCause::Dependencies);
        state
            .get_mut(dependent_dir)
            .expect("bump_at_least inserted the state")
            .dependency_updates
            .insert(target_name, target_new_version);
    }

    for group in ctx.fixed_groups {
        let Some(group_bump) = max_bump_type_of(
            group.iter().filter_map(|dir| state.get(dir).map(|entry| entry.bump_type)),
        ) else {
            continue;
        };
        for dir in group {
            changed |= bump_at_least(state, dir, group_bump, ReleaseCause::Fixed);
        }
    }

    // When the lead crosses to a new stable major, every member re-bases
    // to the band floor. Seed a release for each so the override in
    // apply_epic_band_versions has a version to replace and dependents
    // propagate.
    for epic in ctx.epics {
        if epic_rebase_floor(epic, ctx.participants, new_versions).is_none() {
            continue;
        }
        for member_dir in &epic.member_dirs {
            changed |= bump_at_least(state, member_dir, ReleaseBumpType::Major, ReleaseCause::Epic);
        }
    }
    changed
}

/// The dependents whose declared range no longer accepts a dependency's new
/// version, as `(dependent dir, dependency name, its new version)`.
fn forced_dependency_bumps<'a>(
    participants: &'a BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> Vec<(&'a str, String, String)> {
    let mut forced: Vec<(&str, String, String)> = Vec::new();
    for dependent in participants.values() {
        for dep in &dependent.internal_deps {
            let Some(target_new_version) = new_versions.get(&dep.target_dir) else {
                continue;
            };
            if dependency_still_accepts(participants, dep, target_new_version) {
                continue;
            }
            forced.push((
                dependent.dir.as_str(),
                dep.target_name.clone(),
                target_new_version.clone(),
            ));
        }
    }
    forced
}

/// Whether `dep`'s declared range still accepts its target's new version. A
/// target outside the plan, or a range that does not materialize, constrains
/// nothing.
fn dependency_still_accepts(
    participants: &BTreeMap<String, Participant<'_>>,
    dep: &InternalDep,
    target_new_version: &str,
) -> bool {
    let Some(target) = participants.get(dep.target_dir.as_str()) else {
        return true;
    };
    let Some(materialized) = materialize_workspace_range(dep.spec, target.current_version) else {
        return true;
    };
    range_accepts(&materialized, target_new_version)
}

pub(super) fn planned_releases(
    ctx: &AssembleContext<'_>,
    intents: &PlanIntents<'_>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &BTreeMap<String, String>,
) -> Vec<PlannedRelease> {
    let mut releases: Vec<PlannedRelease> = state
        .iter()
        .map(|(dir, pkg_state)| {
            let participant = &ctx.participants[dir.as_str()];
            PlannedRelease {
                name: participant.name.to_string(),
                dir: dir.clone(),
                root_dir: participant.root_dir.to_path_buf(),
                current_version: participant.current_version.to_string(),
                new_version: match &ctx.opts.snapshot_suffix {
                    Some(suffix) => format!("0.0.0-{suffix}"),
                    None => new_versions[dir].clone(),
                },
                bump_type: pkg_state.bump_type,
                intents: changelog_intents(ctx, intents, dir),
                dependency_updates: pkg_state
                    .dependency_updates
                    .iter()
                    .map(|(dep_name, new_version)| DependencyUpdate {
                        name: dep_name.clone(),
                        new_version: new_version.clone(),
                    })
                    .collect(),
                causes: pkg_state.causes.iter().copied().collect(),
            }
        })
        .collect();
    releases
        .sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.dir.cmp(&right.dir)));
    releases
}

/// The intents this release's changelog covers. A package still on a lane
/// leaves its lane-consumed intents for the graduating release.
fn changelog_intents(
    ctx: &AssembleContext<'_>,
    intents: &PlanIntents<'_>,
    dir: &str,
) -> Vec<ChangeIntent> {
    let mut consumed: Vec<ChangeIntent> = intents
        .pending_by_dir
        .get(dir)
        .map(|intents| intents.iter().map(|&intent| intent.clone()).collect())
        .unwrap_or_default();
    if !ctx.lanes_by_dir.contains_key(dir)
        && let Some(lane_consumed) = intents.lane_consumed_by_dir.get(dir)
    {
        consumed.extend(lane_consumed.iter().map(|&intent| intent.clone()));
    }
    consumed
}

/// A published `package@version` identifies exactly one artifact, so two
/// projects that share a name cannot both release the same version — the
/// registry would reject the second publish, and the name-keyed ledger entry
/// would collide. Caught here, before any manifest is written, naming both
/// directories.
pub(super) fn assert_no_duplicate_release_identity(
    releases: &[PlannedRelease],
) -> Result<(), VersioningError> {
    let mut by_identity: HashMap<String, String> = HashMap::new();
    for release in releases {
        let identity = format!("{}@{}", release.name, release.new_version);
        if let Some(other) = by_identity.insert(identity.clone(), release.dir.clone()) {
            return Err(VersioningError::DuplicateRelease {
                identity,
                first_dir: other,
                second_dir: release.dir.clone(),
            });
        }
    }
    Ok(())
}

fn bump_at_least(
    state: &mut BTreeMap<String, BumpState>,
    dir: &str,
    bump_type: ReleaseBumpType,
    cause: ReleaseCause,
) -> bool {
    let Some(existing) = state.get_mut(dir) else {
        state.insert(
            dir.to_string(),
            BumpState {
                bump_type,
                causes: BTreeSet::from([cause]),
                dependency_updates: BTreeMap::new(),
            },
        );
        return true;
    };
    existing.causes.insert(cause);
    if bump_type > existing.bump_type {
        existing.bump_type = bump_type;
        return true;
    }
    false
}

pub(super) fn collect_pending_intents<'i>(
    ctx: &AssembleContext<'i>,
) -> BTreeMap<String, Vec<&'i ChangeIntent>> {
    let mut pending = BTreeMap::new();
    let empty = PackageConsumption::default();
    for dir in ctx.participants.keys() {
        let consumed = ctx.consumption.get(dir).unwrap_or(&empty);
        let pkg_intents: Vec<&ChangeIntent> = ctx
            .intents
            .iter()
            .filter(|intent| {
                ctx.intent_bump_for(intent, dir).is_some_and(|bump| bump != IntentBumpType::None)
                    && !consumed.all_ids.contains(&intent.id)
            })
            .collect();
        if !pkg_intents.is_empty() {
            pending.insert(dir.clone(), pkg_intents);
        }
    }
    pending
}

/// Intents already consumed by prereleases of a package that has not
/// graduated to a stable version yet. They participate in the cumulative
/// bump computation of the package's lane and compose the stable changelog
/// section at graduation.
pub(super) fn collect_lane_consumed_intents<'i>(
    ctx: &AssembleContext<'i>,
) -> BTreeMap<String, Vec<&'i ChangeIntent>> {
    let mut lane_consumed = BTreeMap::new();
    for dir in ctx.participants.keys() {
        let Some(consumed) = ctx.consumption.get(dir) else {
            continue;
        };
        if consumed.prerelease_only_ids.is_empty() {
            continue;
        }
        let pkg_intents: Vec<&ChangeIntent> = ctx
            .intents
            .iter()
            .filter(|intent| {
                ctx.intent_bump_for(intent, dir).is_some_and(|bump| bump != IntentBumpType::None)
                    && consumed.prerelease_only_ids.contains(&intent.id)
            })
            .collect();
        if !pkg_intents.is_empty() {
            lane_consumed.insert(dir.clone(), pkg_intents);
        }
    }
    lane_consumed
}
