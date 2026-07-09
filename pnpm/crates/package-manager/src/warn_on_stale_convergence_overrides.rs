//! Staleness check for convergence overrides (`"pkg@": "<version>"`).
//!
//! A convergence override's value is derived state — the best version
//! that converges the currently declared ranges. It goes stale when
//! dependents start declaring ranges that admit newer versions: the
//! override then keeps the edges it governs on the old version while
//! newer edges resolve past it, producing the duplication the entry
//! was written to prevent.
//!
//! Must only run after a full resolution: only then has every manifest
//! streamed through the versions overrider, so the collected declared
//! ranges are complete. The frozen-lockfile and optimistic-repeat
//! install paths never run this check.

use crate::overrides::parse_declared_range;
use node_semver::Version;
use pacquet_config_parse_overrides::VersionOverride;
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

/// A convergence override whose value can be raised: `best` is newer
/// than `current_value` and satisfies every declared range of `name`.
pub(crate) struct StaleConvergenceOverride {
    pub name: String,
    pub current_value: String,
    pub best: Version,
}

/// For each convergence override, resolve every collected declared
/// range through `resolve_range` and report the override as stale when
/// a resolved version newer than the override's value satisfies every
/// collected range — a strictly better convergence.
///
/// A range that fails to resolve contributes no candidate but still
/// participates in the satisfies-every-range check, so failures can
/// only suppress the verdict, never fabricate one.
pub(crate) async fn find_stale_convergence_overrides<ResolveRange, ResolveRangeFuture>(
    parsed_overrides: &[VersionOverride],
    converge_declared_ranges: &HashMap<String, HashSet<String>>,
    resolve_range: ResolveRange,
) -> Vec<StaleConvergenceOverride>
where
    ResolveRange: Fn(String, String) -> ResolveRangeFuture,
    ResolveRangeFuture: Future<Output = Option<Version>>,
{
    let mut stale = Vec::new();
    for override_entry in parsed_overrides.iter().filter(|entry| entry.converge) {
        let name = &override_entry.target_pkg.name;
        let Some(ranges) = converge_declared_ranges.get(name) else { continue };
        let Ok(current) = Version::parse(&override_entry.new_bare_specifier) else { continue };
        // The collector only records parseable ranges, so a `None`
        // here is unreachable in practice; bailing out keeps the
        // "satisfies EVERY collected range" guarantee if it ever
        // happens.
        let Some(parsed_ranges) =
            ranges.iter().map(|range| parse_declared_range(range)).collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if parsed_ranges.is_empty() {
            continue;
        }
        let mut candidates = Vec::new();
        for range in ranges {
            if let Some(version) = resolve_range(name.clone(), range.clone()).await {
                candidates.push(version);
            }
        }
        candidates.retain(|candidate| *candidate > current);
        candidates.sort_unstable_by(|lhs, rhs| rhs.cmp(lhs));
        let best = candidates
            .into_iter()
            .find(|candidate| parsed_ranges.iter().all(|range| range.satisfies(candidate)));
        if let Some(best) = best {
            stale.push(StaleConvergenceOverride {
                name: name.clone(),
                current_value: override_entry.new_bare_specifier.clone(),
                best,
            });
        }
    }
    stale
}

/// Resolve the best version `range` admits for `name` through the
/// resolver chain. Metadata is already cached from the resolution that
/// just ran, and the release-age policy carried by `opts` applies, so
/// the answer is a version the resolver would actually pick. `None`
/// when the range fails to resolve or the pick violates a resolution
/// policy.
pub(crate) async fn resolve_best_admitted_version(
    resolver: &dyn Resolver,
    opts: &ResolveOptions,
    name: String,
    range: String,
) -> Option<Version> {
    let wanted = WantedDependency {
        alias: Some(name),
        bare_specifier: Some(range),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, opts).await.ok().flatten()?;
    if result.policy_violation.is_some() {
        return None;
    }
    result.name_ver.map(|name_ver| name_ver.suffix)
}

/// Emit the `pnpm:global` warning for each stale convergence override
/// — the counterpart of pnpm's `globalWarn` emission, parsed by the
/// shared default reporter.
pub(crate) fn warn_stale_convergence_overrides<Reporter: self::Reporter>(
    stale: &[StaleConvergenceOverride],
) {
    for entry in stale {
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: stale_convergence_override_warning(entry),
        }));
    }
}

fn stale_convergence_override_warning(
    StaleConvergenceOverride { name, current_value, best }: &StaleConvergenceOverride,
) -> String {
    format!(
        r#"The convergence override "{name}@": "{current_value}" is stale: every declared range of {name} also admits {best}. Change the override's value to {best} in pnpm-workspace.yaml, or remove the override and run "pnpm dedupe"."#,
    )
}

#[cfg(test)]
mod tests;
