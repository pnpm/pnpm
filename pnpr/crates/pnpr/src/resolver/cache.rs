use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::hash_lockfile;
use sha2::{Digest, Sha256};

use pnpr_policy::Identity;
use pnpr_route::{Footprint, RouteContext};

use super::{
    config_cache::{CONFIG_KEY, ConfigKeyInputs, KeyPart, key_value},
    protocol::ResolveRequest,
};

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
    let Some((candidate_index, _)) = candidates
        .iter()
        .enumerate()
        .find(|(_, candidate)| {
            candidate.footprint.is_public() || candidate.footprint.allows(route_context, identity)
        })
    else {
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
    if let Some(existing) = candidates
        .iter_mut()
        .find(|entry| entry.descriptor_digest == candidate.descriptor_digest)
    {
        *existing = candidate;
        return true;
    }
    candidates.push(candidate);
    enforce_candidate_limit(candidates);
    while count_resolution_candidates(&cache) > MAX_RESOLUTION_CACHE_ENTRIES {
        if !evict_lru_resolution_candidate(&mut cache) {
            break;
        }
    }
    true
}

/// What a finished resolution offers the resolution cache.
pub(super) struct StoreCandidate<'a> {
    pub(super) cache: &'a Mutex<HashMap<String, Vec<CachedResolution>>>,
    pub(super) cache_ttl: Duration,
    pub(super) key: String,
    pub(super) footprint: &'a Mutex<Footprint>,
    pub(super) cache_secret: &'a [u8],
    pub(super) lockfile: &'a Lockfile,
}

/// Offer a finished resolution to the cache, logging what a private one was
/// judged on.
pub(super) fn store_resolution_candidate(candidate: StoreCandidate<'_>) {
    let footprint = candidate.footprint
        .lock()
        .expect("footprint poisoned")
        .clone();
    let descriptor = footprint.digest(candidate.cache_secret);
    let cached = store_resolution(
        candidate.cache,
        candidate.cache_ttl,
        candidate.key,
        footprint.clone(),
        candidate.cache_secret,
        candidate.lockfile,
    );
    if footprint.is_public() {
        return;
    }
    tracing::debug!(
        cached,
        descriptor = descriptor.as_deref().unwrap_or("none"),
        "private resolution cache candidate evaluated",
    );
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
        evict_lru_candidate(candidates);
    }
}

/// Evicts the least recently used candidate, preferring a private one.
fn evict_lru_candidate(candidates: &mut Vec<CachedResolution>) {
    if let Some(index) = candidates
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

fn evict_lru_resolution_candidate(cache: &mut HashMap<String, Vec<CachedResolution>>) -> bool {
    let target = lru_resolution_candidate(cache, CandidateScope::PrivateOnly)
        .or_else(|| lru_resolution_candidate(cache, CandidateScope::Any));
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

/// Which candidates an eviction looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateScope {
    PrivateOnly,
    Any,
}

fn lru_resolution_candidate(
    cache: &HashMap<String, Vec<CachedResolution>>,
    scope: CandidateScope,
) -> Option<(String, usize, Instant)> {
    cache
        .iter()
        .filter_map(|(key, candidates)| {
            candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| match scope {
                    CandidateScope::PrivateOnly => !candidate.footprint.is_public(),
                    CandidateScope::Any => true,
                })
                .min_by_key(|(_, candidate)| candidate.last_used)
                .map(|(index, candidate)| (key.clone(), index, candidate.last_used))
        })
        .min_by_key(|(_, _, last_used)| *last_used)
}

/// The parts the resolution cache key carries beyond [`CONFIG_KEY`].
pub(super) const RESOLUTION_KEY: &[KeyPart] = &[
    // Raw, unlike the config key's parsed view: the frozen fast path reads
    // the request's own value.
    KeyPart { fields: &["overrides"], value: |request, _| serde_json::json!(request.overrides) },
    KeyPart { fields: &["catalogs"], value: |request, _| serde_json::json!(request.catalogs) },
    KeyPart {
        fields: &[
            "projects",
            "dependencies",
            "devDependencies",
            "optionalDependencies",
            "peerDependencies",
        ],
        value: |request, _| serde_json::Value::Array(cache_key_projects(request)),
    },
    KeyPart {
        fields: &["lockfile"],
        value: |request, _| serde_json::json!(request.lockfile.as_ref().map(hash_lockfile)),
    },
    KeyPart {
        fields: &["frozenLockfile"],
        value: |request, _| serde_json::json!(request.frozen_lockfile),
    },
    KeyPart {
        fields: &["preferFrozenLockfile"],
        value: |request, _| serde_json::json!(request.prefer_frozen_lockfile),
    },
    KeyPart {
        fields: &["ignoreManifestCheck"],
        value: |request, _| serde_json::json!(request.ignore_manifest_check),
    },
    KeyPart {
        fields: &["trustLockfile"],
        value: |request, _| serde_json::json!(request.trust_lockfile),
    },
];

pub(super) fn resolution_cache_key(request: &ResolveRequest) -> Option<String> {
    // Explicit metadata refreshes must not repeat an old registry selection
    // from the whole-resolution cache.
    if request.update_patches || request.fix_lockfile {
        return None;
    }
    let inputs = ConfigKeyInputs::for_request(request);
    let input = serde_json::json!([
        key_value(CONFIG_KEY, request, &inputs),
        key_value(RESOLUTION_KEY, request, &inputs),
    ]);
    let bytes = serde_json::to_vec(&input).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}

fn cache_key_projects(request: &ResolveRequest) -> Vec<serde_json::Value> {
    request
        .projects_normalized()
        .into_iter()
        .map(|project| {
            serde_json::json!({
                "dir": project.dir,
                "name": project.name,
                "version": project.version,
                "publishConfig": project.publish_config,
                "dependencies": project.dependencies,
                "devDependencies": project.dev_dependencies,
                "optionalDependencies": project.optional_dependencies,
                "peerDependencies": project.peer_dependencies,
            })
        })
        .collect()
}
