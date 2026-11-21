pub use scope::ExoticSubdepsScope;

mod classification;
mod paths;
mod scope;

use std::{
    cell::OnceCell,
    collections::{
        BTreeMap,
        HashMap,
        HashSet,
    },
    sync::Arc,
};

use pnpm_lockfile::{
    BoundProviders,
    Lockfile,
    LockfileEntries,
    PackageKey,
    PackageMetadata,
    PeerBindings,
    PkgName,
    SnapshotDepRef,
    SnapshotEntry,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;

use self::{
    classification::{
        exotic_resolved_via,
        fallback_resolution,
        is_registry_key,
    },
    scope::{
        collect_allowed_peer_providers,
        collect_workspace_directories,
        is_workspace_snapshot,
    },
};
use super::{
    ReplacedCandidate,
    ReplacedEntries,
};

pub const EXOTIC_SUBDEP_VIOLATION_CODE: &str = "EXOTIC_SUBDEP";

/// Whether `blockExoticSubdeps` is enforced on lockfile subdependencies.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum ExoticSubdepsPolicy {
    /// Exotic subdependencies are permitted (`blockExoticSubdeps: false` or `trustLockfile: true`).
    #[default]
    Allow,
    /// Exotic subdependencies are rejected (`blockExoticSubdeps: true`).
    Block(Arc<ExoticSubdepsScope>),
}

impl ExoticSubdepsPolicy {
    #[must_use]
    pub fn from_config(block_exotic_subdeps: bool) -> Self {
        if block_exotic_subdeps { Self::block_unscoped() } else { Self::Allow }
    }

    #[must_use]
    pub fn block_unscoped() -> Self {
        Self::Block(Arc::new(ExoticSubdepsScope::default()))
    }

    #[must_use]
    pub fn block(scope: ExoticSubdepsScope) -> Self {
        Self::Block(Arc::new(scope))
    }

    #[must_use]
    pub const fn is_enforced(&self) -> bool {
        matches!(self, Self::Block(_))
    }

    #[must_use]
    pub fn scope(&self) -> Option<&ExoticSubdepsScope> {
        match self {
            Self::Allow => None,
            Self::Block(scope) => Some(scope),
        }
    }
}

pub(super) struct ExoticSubdepScan {
    pub(super) violations: Vec<ResolutionPolicyViolation>,
    pub(super) skipped_replaced: bool,
}

struct ScanContext<'a> {
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    scope: &'a ExoticSubdepsScope,
    workspace_dirs: HashSet<String>,
    allowed_peer_providers: HashSet<PackageKey>,
    entries: LockfileEntries<'a>,
    replaced: Option<ReplacedEntries<'a>>,
    violations: BTreeMap<String, ResolutionPolicyViolation>,
    skipped_replaced: bool,
}

struct ParentPeerContext<'a> {
    parent_key: &'a PackageKey,
    parent_meta: Option<&'a PackageMetadata>,
    has_peer_suffix: bool,
    bound_peers: OnceCell<Option<BoundProviders>>,
}

pub(super) fn collect_exotic_subdep_violations(
    lockfile: &Lockfile,
    replaced: Option<ReplacedEntries<'_>>,
    scope: &ExoticSubdepsScope,
) -> ExoticSubdepScan {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return ExoticSubdepScan { violations: Vec::new(), skipped_replaced: false };
    };
    let workspace_dirs = collect_workspace_directories(lockfile, scope);
    let allowed_peer_providers = collect_allowed_peer_providers(lockfile, scope, &workspace_dirs);
    let mut ctx = ScanContext {
        packages: lockfile.packages.as_ref(),
        scope,
        workspace_dirs,
        allowed_peer_providers,
        entries: LockfileEntries::from(lockfile),
        replaced,
        violations: BTreeMap::new(),
        skipped_replaced: false,
    };

    for (parent_key, snapshot) in snapshots {
        scan_parent_snapshot(&mut ctx, parent_key, snapshot);
    }

    ExoticSubdepScan {
        violations: ctx.violations.into_values().collect(),
        skipped_replaced: ctx.skipped_replaced,
    }
}

fn scan_parent_snapshot(
    ctx: &mut ScanContext<'_>,
    parent_key: &PackageKey,
    snapshot: &SnapshotEntry,
) {
    if is_workspace_snapshot(parent_key, ctx.packages, &ctx.workspace_dirs) {
        return;
    }
    let parent = ParentPeerContext {
        parent_key,
        parent_meta: metadata_for(ctx.packages, parent_key),
        has_peer_suffix: PeerBindings::from_key(parent_key).is_some(),
        bound_peers: OnceCell::new(),
    };
    for (alias, dep_ref) in snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
    {
        scan_dependency_edge(ctx, &parent, alias, dep_ref);
    }
}

fn scan_dependency_edge(
    ctx: &mut ScanContext<'_>,
    parent: &ParentPeerContext<'_>,
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
) {
    let Some(child_key) = dep_ref.resolve(alias) else { return };
    let child_meta = metadata_for(ctx.packages, &child_key);
    let Some(resolved_via) = exotic_resolved_via(alias, &child_key, child_meta, ctx.scope) else {
        return;
    };
    if is_peer_satisfaction_edge(
        alias,
        &child_key,
        parent,
        ctx.entries,
        &ctx.allowed_peer_providers,
    ) {
        return;
    }
    if is_edge_replaced(ctx.replaced, parent.parent_key, &child_key) {
        ctx.skipped_replaced = true;
        return;
    }
    insert_exotic_violation(&mut ctx.violations, &child_key, child_meta, resolved_via);
}

fn is_edge_replaced(
    replaced: Option<ReplacedEntries<'_>>,
    parent_key: &PackageKey,
    child_key: &PackageKey,
) -> bool {
    let Some(ReplacedEntries(is_replaced)) = replaced else { return false };
    key_is_replaced(parent_key, is_replaced) || key_is_replaced(child_key, is_replaced)
}

fn insert_exotic_violation(
    violations: &mut BTreeMap<String, ResolutionPolicyViolation>,
    child_key: &PackageKey,
    child_meta: Option<&PackageMetadata>,
    resolved_via: &'static str,
) {
    let name = child_key.name.clone();
    let version = child_meta
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| child_key.suffix.version().to_string());
    let resolution = child_meta
        .map(|meta| meta.resolution.clone())
        .unwrap_or_else(|| fallback_resolution(child_key));
    let resolution_json = serde_json::to_string(&resolution)
        .expect("LockfileResolution must serialize for exotic subdep violation dedupe");
    let dedupe_key = format!("{name}@{version}@{resolution_json}");
    violations
        .entry(dedupe_key)
        .or_insert_with(|| ResolutionPolicyViolation {
            name,
            version,
            resolution,
            code: EXOTIC_SUBDEP_VIOLATION_CODE,
            reason: format!(
                "(resolved via {resolved_via}) is not allowed in subdependencies when blockExoticSubdeps is enabled",
            ),
        });
}

pub(super) fn metadata_for<'a>(
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    key: &PackageKey,
) -> Option<&'a PackageMetadata> {
    let packages = packages?;
    packages
        .get(&key.without_peer())
        .or_else(|| packages.get(key))
}

fn is_peer_satisfaction_edge(
    alias: &PkgName,
    child_key: &PackageKey,
    parent: &ParentPeerContext<'_>,
    entries: LockfileEntries<'_>,
    allowed_peer_providers: &HashSet<PackageKey>,
) -> bool {
    if !parent.has_peer_suffix || !allowed_peer_providers.contains(child_key) {
        return false;
    }
    let declared_peer = parent.parent_meta
        .and_then(|meta| meta.peer_dependencies.as_ref())
        .is_some_and(|peers| peers.contains_key(&alias.to_string()));
    if !declared_peer {
        return false;
    }
    parent.bound_peers
        .get_or_init(|| Some(BoundProviders::of_snapshot(entries, parent.parent_key)))
        .as_ref()
        .and_then(|bound| bound.provider_keys(child_key))
        .is_some_and(|keys| keys.contains(child_key) || keys.contains(&child_key.without_peer()))
}

fn key_is_replaced(
    key: &PackageKey,
    is_replaced: &(dyn Fn(ReplacedCandidate<'_>) -> bool + Send + Sync),
) -> bool {
    let bare = key.without_peer();
    if !is_registry_key(&bare) {
        return false;
    }
    let registry_name = bare.suffix.registry_qualified().map(|(name, _)| name);
    let version = match bare.suffix.registry_qualified() {
        Some((_, version)) => version.to_string(),
        None => bare.suffix.version().to_string(),
    };
    is_replaced(ReplacedCandidate { name: &bare.name, version: &version, registry_name })
}
