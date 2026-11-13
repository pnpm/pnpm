use indexmap::IndexMap;
use pnpm_config::{Config, TrustPolicy};
use pnpm_crypto_hash::create_short_hash;
use pnpm_package_is_installable::SupportedArchitectures;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Inputs that determine which cached `dlx` install directory an invocation
/// may reuse.
#[derive(Clone, Copy)]
pub(in crate::cli_args::dlx) struct CacheKeyInputs<'a> {
    pub pkgs: &'a [String],
    pub registries: &'a BTreeMap<String, String>,
    pub allow_build: &'a [(&'a str, bool)],
    pub supported_architectures: Option<&'a SupportedArchitectures>,
    pub engine: Option<&'a str>,
    pub policy: CachePolicyInputs<'a>,
}

/// Resolution and supply-chain security settings that constrain what the
/// cached `dlx` install is permitted to contain.
#[derive(Clone, Copy)]
pub(in crate::cli_args::dlx) struct CachePolicyInputs<'a> {
    pub overrides: Option<&'a IndexMap<String, String>>,
    pub minimum_release_age: Option<MinimumReleaseAgeInput<'a>>,
    pub trust_policy: Option<TrustPolicyInput<'a>>,
    pub trust_lockfile: bool,
    pub block_exotic_subdeps: bool,
}

impl Default for CachePolicyInputs<'_> {
    fn default() -> Self {
        Self {
            overrides: None,
            minimum_release_age: None,
            trust_policy: None,
            trust_lockfile: false,
            block_exotic_subdeps: true,
        }
    }
}

impl<'a> CachePolicyInputs<'a> {
    pub(super) fn from_config(config: &'a Config) -> Self {
        let overrides = config.overrides
            .as_ref()
            .filter(|overrides| !overrides.is_empty());
        let minimum_release_age = config.minimum_release_age
            .filter(|&minutes| minutes > 0)
            .map(|minutes| MinimumReleaseAgeInput {
                minutes,
                exclude: config.minimum_release_age_exclude
                    .as_deref()
                    .unwrap_or(&[]),
                strict: config.resolved_minimum_release_age_strict(),
                ignore_missing_time: config.minimum_release_age_ignore_missing_time,
            });
        let trust_policy = match config.trust_policy {
            TrustPolicy::Off => None,
            TrustPolicy::NoDowngrade => Some(TrustPolicyInput {
                exclude: config.trust_policy_exclude
                    .as_deref()
                    .unwrap_or(&[]),
                ignore_after: config.trust_policy_ignore_after,
            }),
        };
        Self {
            overrides,
            minimum_release_age,
            trust_policy,
            trust_lockfile: config.trust_lockfile,
            block_exotic_subdeps: config.block_exotic_subdeps,
        }
    }
}

/// Active `minimumReleaseAge` policy inputs for [`CachePolicyInputs`].
#[derive(Clone, Copy)]
pub(in crate::cli_args::dlx) struct MinimumReleaseAgeInput<'a> {
    pub minutes: u64,
    pub exclude: &'a [String],
    pub strict: bool,
    pub ignore_missing_time: bool,
}

/// Active `trustPolicy: 'no-downgrade'` inputs for [`CachePolicyInputs`].
#[derive(Clone, Copy)]
pub(in crate::cli_args::dlx) struct TrustPolicyInput<'a> {
    pub exclude: &'a [String],
    pub ignore_after: Option<u64>,
}

/// Build the `dlx` cache key from the sorted package specs, sorted
/// registries, normalized `--allow-build` selectors, each non-empty
/// `supportedArchitectures` axis, active resolution/security policies, and
/// the Node.js engine name.
///
/// Keys on the raw specs (after `catalog:` expansion) and uses
/// [`create_short_hash`]; the `dlx` caches are not shared across major CLI
/// implementations, so the key format is not a cross-tool contract.
pub(in crate::cli_args::dlx) fn create_cache_key(inputs: &CacheKeyInputs<'_>) -> String {
    let mut sorted_pkgs: Vec<&str> = inputs.pkgs
        .iter()
        .map(String::as_str)
        .collect();
    sorted_pkgs.sort_unstable();
    let registry_pairs: Vec<(&str, &str)> = inputs.registries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut args = vec![json!(sorted_pkgs), json!(registry_pairs)];
    let normalized_allow = normalize_allow_build_selectors(inputs.allow_build);
    if !normalized_allow.is_empty() {
        args.push(json!({ "allowBuild": normalized_allow }));
    }
    args.extend(architecture_key_inputs(inputs.supported_architectures));
    args.extend(policy_key_inputs(&inputs.policy));
    // `null` when no `node` is found, which still differs from a key that
    // records no engine at all.
    args.push(json!({ "engine": inputs.engine }));
    create_short_hash(&serde_json::to_string(&args).expect("serialize cache key inputs"))
}

/// Canonicalize `--allow-build` selectors: when the same package name is
/// given more than once, the last verdict wins (matching `allow_builds` map
/// insertion), and the effective entries are sorted by code unit.
fn normalize_allow_build_selectors(allow_build: &[(&str, bool)]) -> Vec<String> {
    let mut verdicts = BTreeMap::<&str, bool>::new();
    for &(name, allowed) in allow_build {
        verdicts.insert(name, allowed);
    }
    let mut formatted: Vec<String> = verdicts
        .into_iter()
        .map(|(name, allowed)| if allowed { name.to_string() } else { format!("!{name}") })
        .collect();
    formatted.sort_unstable();
    formatted
}

fn policy_key_inputs(policy: &CachePolicyInputs<'_>) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(overrides) = policy.overrides.filter(|map| !map.is_empty()) {
        let mut pairs: Vec<(&str, &str)> = overrides
            .iter()
            .map(|(selector, spec)| (selector.as_str(), spec.as_str()))
            .collect();
        pairs.sort_unstable_by_key(|(left, _)| *left);
        items.push(json!({ "overrides": pairs }));
    }
    if let Some(release_age) = &policy.minimum_release_age {
        items.push(minimum_release_age_json(release_age));
    }
    if let Some(trust) = &policy.trust_policy {
        items.push(trust_policy_json(trust));
    }
    if policy.trust_lockfile {
        items.push(json!({ "trustLockfile": true }));
    }
    if !policy.block_exotic_subdeps {
        items.push(json!({ "blockExoticSubdeps": false }));
    }
    items
}

fn minimum_release_age_json(release_age: &MinimumReleaseAgeInput<'_>) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("minutes".to_string(), json!(release_age.minutes));
    entry.insert("strict".to_string(), json!(release_age.strict));
    entry.insert("ignoreMissingTime".to_string(), json!(release_age.ignore_missing_time));
    if !release_age.exclude.is_empty() {
        entry.insert("exclude".to_string(), json!(sorted_once(release_age.exclude)));
    }
    json!({ "minimumReleaseAge": entry })
}

fn trust_policy_json(trust: &TrustPolicyInput<'_>) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("policy".to_string(), json!("no-downgrade"));
    if !trust.exclude.is_empty() {
        entry.insert("exclude".to_string(), json!(sorted_once(trust.exclude)));
    }
    if let Some(ignore_after) = trust.ignore_after {
        entry.insert("ignoreAfter".to_string(), json!(ignore_after));
    }
    json!({ "trustPolicy": entry })
}

/// What the platforms an install prepares for contribute to its cache key.
///
/// The axes are recorded one axis at a time, each named once and sorted,
/// since rewriting them in another order asks for the same install.
///
/// A platform list is recorded as the platforms it resolves to, in the
/// order it named them. The order matters because which platform comes
/// first decides which runtime archive the install takes when this
/// machine is none of them, and resolving matters because `current` is a
/// different platform on each machine while the word is the same.
fn architecture_key_inputs(supported: Option<&SupportedArchitectures>) -> Vec<Value> {
    match supported {
        None => Vec::new(),
        Some(SupportedArchitectures::Axes(axes)) => {
            [("cpu", &axes.cpu), ("libc", &axes.libc), ("os", &axes.os)]
                .into_iter()
                .filter_map(|(key, values)| {
                    let values = values
                        .as_ref()
                        .filter(|values| !values.is_empty())?;
                    Some(json!({ "supportedArchitectures": { key: sorted_once(values) } }))
                })
                .collect()
        }
        Some(supported @ SupportedArchitectures::Platforms(_)) => {
            let named: Vec<String> = supported
                .host_platforms()
                .iter()
                .map(ToString::to_string)
                .collect();
            vec![json!({ "supportedArchitectures": named })]
        }
    }
}

/// The values as the cache key records them: named once, in an order
/// rewriting the configuration cannot change.
fn sorted_once(values: &[String]) -> Vec<&str> {
    let mut deduped: Vec<&str> = values
        .iter()
        .map(String::as_str)
        .collect();
    deduped.sort_unstable();
    deduped.dedup();
    deduped
}
