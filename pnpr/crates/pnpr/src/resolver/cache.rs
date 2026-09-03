use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::hash_lockfile;
use sha2::{Digest, Sha256};

use pnpr_policy::Identity;
use pnpr_route::{Footprint, RouteContext};

use super::protocol::ResolveRequest;

pub(super) struct CachedResolution {
    lockfile: Lockfile,
    inserted: Instant,
    last_used: Instant,
    pub(super) footprint: Footprint,
    descriptor_digest: Option<String>,
}

pub(super) const MAX_RESOLUTION_CACHE_ENTRIES: usize = 1024;
pub(super) const MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY: usize = 8;

pub(super) fn cached_resolution(
    cache: &Mutex<HashMap<String, Vec<CachedResolution>>>,
    ttl: Duration,
    key: &str,
    route_context: &RouteContext,
    identity: &Identity,
) -> Option<Lockfile> {
    if ttl.is_zero() {
        return None;
    }
    let mut cache = cache.lock().expect("resolution cache poisoned");
    let candidates = cache.get_mut(key)?;
    candidates.retain(|candidate| candidate.inserted.elapsed() <= ttl);
    let Some((candidate_index, _)) = candidates.iter().enumerate().find(|(_, candidate)| {
        candidate.footprint.is_public() || candidate.footprint.allows(route_context, identity)
    }) else {
        if candidates.is_empty() {
            cache.remove(key);
        }
        return None;
    };
    let candidate = &mut candidates[candidate_index];
    candidate.last_used = Instant::now();
    Some(candidate.lockfile.clone())
}

pub(super) fn store_resolution(
    cache: &Mutex<HashMap<String, Vec<CachedResolution>>>,
    ttl: Duration,
    key: String,
    footprint: Footprint,
    secret: &[u8],
    lockfile: &Lockfile,
) -> bool {
    if ttl.is_zero() {
        return false;
    }
    let now = Instant::now();
    let descriptor_digest = footprint.digest(secret);
    let candidate = CachedResolution {
        lockfile: lockfile.clone(),
        inserted: now,
        last_used: now,
        footprint,
        descriptor_digest,
    };
    let mut cache = cache.lock().expect("resolution cache poisoned");
    prune_expired_resolution_cache(&mut cache, ttl);
    let candidates = cache.entry(key).or_default();
    if let Some(existing) =
        candidates.iter_mut().find(|entry| entry.descriptor_digest == candidate.descriptor_digest)
    {
        *existing = candidate;
        return true;
    }
    candidates.push(candidate);
    enforce_candidate_limit(candidates);
    while count_resolution_candidates(&cache) > MAX_RESOLUTION_CACHE_ENTRIES {
        if !evict_lru_resolution_candidate(&mut cache, true) {
            break;
        }
    }
    true
}

fn prune_expired_resolution_cache(
    cache: &mut HashMap<String, Vec<CachedResolution>>,
    ttl: Duration,
) {
    cache.retain(|_, candidates| {
        candidates.retain(|candidate| candidate.inserted.elapsed() <= ttl);
        !candidates.is_empty()
    });
}

fn enforce_candidate_limit(candidates: &mut Vec<CachedResolution>) {
    while candidates.len() > MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY {
        evict_lru_candidate(candidates, true);
    }
}

fn evict_lru_candidate(candidates: &mut Vec<CachedResolution>, private_first: bool) {
    if private_first
        && let Some(index) = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| !candidate.footprint.is_public())
            .min_by_key(|(_, candidate)| candidate.last_used)
            .map(|(index, _)| index)
    {
        candidates.remove(index);
        return;
    }
    if let Some(index) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.last_used)
        .map(|(index, _)| index)
    {
        candidates.remove(index);
    }
}

fn count_resolution_candidates(cache: &HashMap<String, Vec<CachedResolution>>) -> usize {
    cache.values().map(Vec::len).sum()
}

fn evict_lru_resolution_candidate(
    cache: &mut HashMap<String, Vec<CachedResolution>>,
    private_first: bool,
) -> bool {
    let target = lru_resolution_candidate(cache, private_first)
        .or_else(|| if private_first { lru_resolution_candidate(cache, false) } else { None });
    let Some((key, index, _)) = target else {
        return false;
    };
    if let Some(candidates) = cache.get_mut(&key)
        && index < candidates.len()
    {
        candidates.remove(index);
    }
    if cache.get(&key).is_some_and(Vec::is_empty) {
        cache.remove(&key);
    }
    true
}

fn lru_resolution_candidate(
    cache: &HashMap<String, Vec<CachedResolution>>,
    private_only: bool,
) -> Option<(String, usize, Instant)> {
    cache
        .iter()
        .filter_map(|(key, candidates)| {
            candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| !private_only || !candidate.footprint.is_public())
                .min_by_key(|(_, candidate)| candidate.last_used)
                .map(|(index, candidate)| (key.clone(), index, candidate.last_used))
        })
        .min_by_key(|(_, _, last_used)| *last_used)
}

pub(super) fn resolution_cache_key(
    config: &PacquetConfig,
    request: &ResolveRequest,
) -> Option<String> {
    // Explicit metadata refreshes must not repeat an old registry selection
    // from the whole-resolution cache.
    if request.update_patches || request.fix_lockfile {
        return None;
    }
    let projects: Vec<serde_json::Value> = request
        .projects_normalized()
        .into_iter()
        .map(|project| {
            serde_json::json!({
                "dir": project.dir,
                "name": project.name,
                "version": project.version,
                "dependencies": project.dependencies,
                "devDependencies": project.dev_dependencies,
                "optionalDependencies": project.optional_dependencies,
            })
        })
        .collect();
    let input = serde_json::json!({
        "registry": &config.registry,
        "registries": &request.registries,
        "overrides": &request.overrides,
        "catalogs": &request.catalogs,
        "patchedDependencies": &request.patched_dependencies,
        "packageExtensions": &request.package_extensions,
        "allowUnusedPatches": request.allow_unused_patches,
        "autoInstallPeers": config.auto_install_peers,
        "dedupePeers": config.dedupe_peers,
        "excludeLinksFromLockfile": config.exclude_links_from_lockfile,
        "projects": projects,
        "inputLockfileHash": request.lockfile.as_ref().map(hash_lockfile),
        "frozenLockfile": request.frozen_lockfile,
        "preferFrozenLockfile": request.prefer_frozen_lockfile,
        "ignoreManifestCheck": request.ignore_manifest_check,
        "trustLockfile": request.trust_lockfile,
        "resolutionMode": request.resolution_mode,
        "minimumReleaseAge": request.minimum_release_age,
        "minimumReleaseAgeExclude": &request.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
        "trustPolicy": request.trust_policy,
        "trustPolicyExclude": &request.trust_policy_exclude,
        "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
    });
    let bytes = serde_json::to_vec(&input).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}
