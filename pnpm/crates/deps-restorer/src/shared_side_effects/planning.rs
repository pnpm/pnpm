use super::{
    ApplySharedSideEffectsOptions, BaseCasPaths, RemoteCacheSetup, dependency_package,
    insert_side_effects_map, package_version, patch_hash, stored_remote_side_effects_are_verified,
    stored_remote_side_effects_blobs_are_valid,
};
use crate::{
    AllowBuildPolicy, RequiresBuildBySnapshot, SideEffectsBySnapshot, SideEffectsMapsBySnapshot,
    StoreIndexKeysBySnapshot, build_deps_subgraph, deps_graph::in_lockfile_order,
};
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_pnpr_client::{ArtifactCandidate, ArtifactSubject, OwnerScope, PackageIdentity};
use pnpm_store_dir::SideEffectsDiff;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

pub(super) struct CandidateGroup {
    pub(super) candidate: ArtifactCandidate,
    pub(super) snapshots: Vec<(PackageKey, String, String)>,
}
pub(super) fn plan_eligible_roots(
    options: &ApplySharedSideEffectsOptions<'_>,
    setup: &RemoteCacheSetup,
) -> Vec<PackageKey> {
    let roots = eligible_roots(
        options.snapshots,
        options.requires_build_by_snapshot,
        options.allow_build_policy,
        options.base_cas_paths,
        &setup.eligible_packages,
    );
    tracing::debug!(
        target: "pacquet::install",
        eligible_snapshots = roots.len(),
        "planned remote side-effects candidates",
    );
    roots
}
/// The snapshots whose built output the remote cache may supply: an
/// eligible package with a build to run, an explicit allow-build
/// verdict, and a materialized base file map to overlay.
pub(super) fn eligible_roots(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    requires_build_by_snapshot: &RequiresBuildBySnapshot,
    allow_build_policy: &AllowBuildPolicy,
    base_cas_paths: &BaseCasPaths,
    eligible_packages: &HashSet<String>,
) -> Vec<PackageKey> {
    in_lockfile_order(snapshots)
        .into_iter()
        .filter(|(snapshot_key, _)| {
            requires_build_by_snapshot.get(*snapshot_key).copied().unwrap_or(false)
                && eligible_packages.contains(&snapshot_key.name.to_string())
                && allow_build_policy.check(&snapshot_key.without_peer().to_string()) == Some(true)
                && base_cas_paths.contains_key(*snapshot_key)
        })
        .map(|(snapshot_key, _)| snapshot_key.clone())
        .collect()
}
/// The read-only inputs of [`plan_candidate_groups`].
pub(super) struct CandidatePlan<'a> {
    pub(super) config: &'a Config,
    pub(super) snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub(super) packages: &'a HashMap<PackageKey, PackageMetadata>,
    pub(super) setup: &'a RemoteCacheSetup,
    pub(super) side_effects_by_snapshot: &'a SideEffectsBySnapshot,
    pub(super) store_index_keys_by_snapshot: &'a StoreIndexKeysBySnapshot,
}
/// Group the eligible snapshots by artifact input key, dropping the
/// ones a persisted or locally cached overlay already covers.
///
/// Two snapshots that hash to one input key but describe different
/// subjects are a collision: neither may be looked up, because the
/// server answers per key.
pub(super) async fn plan_candidate_groups(
    plan: &CandidatePlan<'_>,
    roots: Vec<PackageKey>,
    mut persisted_remote: HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> BTreeMap<String, CandidateGroup> {
    let mut hasher = DepStateHasher::new(plan, &roots);
    let mut groups = BTreeMap::<String, CandidateGroup>::new();
    let mut collisions = HashSet::new();
    for snapshot_key in roots {
        let patch_hash = patch_hash(&snapshot_key);
        let input_key = hasher.input_key(&snapshot_key, patch_hash.as_deref());
        if collisions.contains(&input_key) {
            continue;
        }
        let Some(planned) = plan_root(
            plan,
            &mut hasher,
            RootKeys {
                snapshot_key: &snapshot_key,
                input_key: &input_key,
                patch_hash: patch_hash.as_deref(),
            },
            &mut persisted_remote,
            side_effects_maps_by_snapshot,
        )
        .await
        else {
            continue;
        };
        group_candidate(
            &mut groups,
            &mut collisions,
            input_key,
            planned.candidate,
            (snapshot_key, planned.local_cache_key, planned.store_index_key),
        );
    }
    groups
}
/// The dep graph the eligible roots hash over, with its memo and the
/// install's engine string.
pub(super) struct DepStateHasher {
    graph: HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>,
    cache: pnpm_graph_hasher::DepsStateCache<PackageKey>,
    engine_name: String,
}
impl DepStateHasher {
    fn new(plan: &CandidatePlan<'_>, roots: &[PackageKey]) -> Self {
        let graph = build_deps_subgraph(plan.snapshots, plan.packages, roots.iter().cloned());
        let mut cache = pnpm_graph_hasher::DepsStateCache::new();
        pnpm_graph_hasher::warm_deps_state_cache(
            &graph,
            &mut cache,
            in_lockfile_order(&graph).into_iter().map(|(key, _)| key),
        );
        Self {
            graph,
            cache,
            engine_name: pnpm_graph_hasher::engine_name(plan.setup.node_major, None, None),
        }
    }

    fn input_key(&self, snapshot_key: &PackageKey, patch_hash: Option<&str>) -> String {
        pnpm_graph_hasher::calc_dep_state_input_key(&self.graph, snapshot_key, patch_hash)
    }

    fn local_cache_key(&mut self, snapshot_key: &PackageKey, patch_hash: Option<&str>) -> String {
        pnpm_graph_hasher::calc_dep_state(
            &self.graph,
            &mut self.cache,
            snapshot_key,
            &pnpm_graph_hasher::CalcDepStateOptions {
                engine_name: &self.engine_name,
                patch_file_hash: patch_hash,
                include_dep_graph_hash: true,
            },
        )
    }
}
pub(super) struct RootKeys<'r> {
    snapshot_key: &'r PackageKey,
    input_key: &'r str,
    patch_hash: Option<&'r str>,
}
pub(super) struct PlannedRoot {
    candidate: ArtifactCandidate,
    local_cache_key: String,
    store_index_key: String,
}
/// `None` when a persisted or locally cached overlay already covers the
/// root, or the store index holds no row for it.
pub(super) async fn plan_root(
    plan: &CandidatePlan<'_>,
    hasher: &mut DepStateHasher,
    root: RootKeys<'_>,
    persisted_remote: &mut HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> Option<PlannedRoot> {
    let candidate =
        artifact_candidate(plan, root.snapshot_key, root.input_key, plan.setup.owner.clone())?;
    let local_cache_key = hasher.local_cache_key(root.snapshot_key, root.patch_hash);
    if reuse_persisted_overlay(
        plan,
        &candidate,
        root.snapshot_key,
        &local_cache_key,
        persisted_remote,
        side_effects_maps_by_snapshot,
    )
    .await
    {
        return None;
    }
    if plan.config.side_effects_cache_read()
        && side_effects_maps_by_snapshot
            .get(root.snapshot_key)
            .is_some_and(|maps| maps.contains_key(&local_cache_key))
    {
        return None;
    }
    let store_index_key = plan.store_index_keys_by_snapshot.get(root.snapshot_key).cloned()?;
    Some(PlannedRoot { candidate, local_cache_key, store_index_key })
}
/// The artifact one snapshot would look up. `None` when the package has
/// no metadata row, or no integrity to bind the artifact's subject to.
pub(super) fn artifact_candidate(
    plan: &CandidatePlan<'_>,
    snapshot_key: &PackageKey,
    input_key: &str,
    owner: OwnerScope,
) -> Option<ArtifactCandidate> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = plan.packages.get(&metadata_key)?;
    let source_integrity = metadata.resolution.checkable_integrity().map(ToString::to_string)?;
    Some(ArtifactCandidate {
        key: input_key.to_owned(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity {
                name: metadata_key.name.to_string(),
                version: package_version(&metadata_key, metadata.version.as_deref()),
            },
            source_integrity,
        ),
        owner,
    })
}
/// Whether a previous install's persisted overlay still stands for this
/// snapshot: its stored diff verifies against the candidate and every
/// blob it names is still in the store. A reused overlay is re-inserted
/// into the live maps and needs no lookup.
pub(super) async fn reuse_persisted_overlay(
    plan: &CandidatePlan<'_>,
    candidate: &ArtifactCandidate,
    snapshot_key: &PackageKey,
    local_cache_key: &str,
    persisted_remote: &mut HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> bool {
    let Some(overlay) =
        persisted_remote.remove(&(snapshot_key.clone(), local_cache_key.to_owned()))
    else {
        return false;
    };
    let diff = verified_persisted_diff(plan, candidate, snapshot_key, local_cache_key);
    let Some(diff) = diff else {
        return false;
    };
    match stored_remote_side_effects_blobs_are_valid(diff, &overlay).await {
        Ok(true) => {
            insert_side_effects_map(
                side_effects_maps_by_snapshot,
                snapshot_key.clone(),
                local_cache_key.to_owned(),
                overlay,
            );
            true
        }
        Ok(false) => false,
        // An artifact that cannot be checked is not looked up remotely
        // either: the local build stands in for it.
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                package = %dependency_package(candidate).name,
                %error,
                "persisted remote side-effects artifact could not be checked",
            );
            true
        }
    }
}
pub(super) fn verified_persisted_diff<'a>(
    plan: &'a CandidatePlan<'_>,
    candidate: &ArtifactCandidate,
    snapshot_key: &PackageKey,
    local_cache_key: &str,
) -> Option<&'a SideEffectsDiff> {
    plan.side_effects_by_snapshot
        .get(snapshot_key)
        .and_then(|diffs| diffs.get(local_cache_key))
        .filter(|diff| {
            stored_remote_side_effects_are_verified(
                diff,
                candidate,
                plan.config.pnpr_server.as_deref(),
                &plan.setup.supported_tags,
                &plan.setup.trusted_keys,
            )
        })
}
/// Add the candidate to its input key's group. Two snapshots that share
/// an input key but not a subject cancel the key outright: the group is
/// dropped and the key remembered so later snapshots skip it too.
pub(super) fn group_candidate(
    groups: &mut BTreeMap<String, CandidateGroup>,
    collisions: &mut HashSet<String>,
    input_key: String,
    candidate: ArtifactCandidate,
    entry: (PackageKey, String, String),
) {
    let Some(group) = groups.get_mut(&input_key) else {
        groups.insert(input_key, CandidateGroup { candidate, snapshots: vec![entry] });
        return;
    };
    if group.candidate.subject != candidate.subject {
        groups.remove(&input_key);
        collisions.insert(input_key);
        return;
    }
    group.snapshots.push(entry);
}
