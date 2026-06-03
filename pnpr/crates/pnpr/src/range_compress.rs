//! Shrink the abbreviated metadata further by rewriting each semver
//! range in a version's dependency maps to the shortest *equivalent*
//! string. `^1.0.0` becomes `1`, `~1.2.0` becomes `1.2`,
//! `>=1.2.3 <2.0.0` becomes `^1.2.3`, and so on — the resolver sees
//! the identical version set, just fewer bytes on the wire.
//!
//! ## Why this is safe
//!
//! [`node_semver::Range`] normalizes every range to a canonical set of
//! interval bounds and derives [`PartialEq`] over that representation,
//! so `Range::parse(a) == Range::parse(b)` is exact version-set
//! equality. [`compress_range`] proposes shorter candidate strings and
//! only accepts one whose parsed range is `==` the original's. A
//! candidate that would shift the version set — or any value that
//! isn't a plain semver range at all (a `git+https`, `npm:alias`,
//! `workspace:*`, dist-tag, or tarball-URL specifier) — fails the
//! check or fails to parse, and the original string is kept. The
//! transform can only ever make a range shorter, never change what it
//! resolves to.

use node_semver::Range;
use serde_json::Value;

/// Version-object fields whose values are semver ranges keyed by
/// dependency name. `bundleDependencies` (an array of names) and
/// `peerDependenciesMeta` (per-dep flags) carry no ranges, so they're
/// excluded.
const DEPENDENCY_FIELDS: &[&str] = &["dependencies", "peerDependencies", "optionalDependencies"];

/// Compress the dependency ranges of every version in a full packument
/// in place. Used at publish time so packages hosted in pnpr are
/// stored already-compressed.
pub fn compress_packument_dependencies(packument: &mut Value) {
    let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for version in versions.values_mut() {
        compress_version_dependencies(version);
    }
}

/// Compress the dependency ranges of a single version object in place.
pub fn compress_version_dependencies(version: &mut Value) {
    let Some(obj) = version.as_object_mut() else { return };
    for &field in DEPENDENCY_FIELDS {
        let Some(deps) = obj.get_mut(field).and_then(Value::as_object_mut) else {
            continue;
        };
        for spec in deps.values_mut() {
            let Some(range) = spec.as_str() else { continue };
            if let Some(shorter) = compress_range(range) {
                *spec = Value::String(shorter);
            }
        }
    }
}

/// Return a strictly-shorter string denoting the identical version set
/// as `range`, or `None` when no shorter equivalent is found (or
/// `range` isn't a parseable semver range). Every returned candidate
/// is verified to parse back to a range `==` the original, so the
/// result is always a safe, drop-in replacement.
pub fn compress_range(range: &str) -> Option<String> {
    let parsed = Range::parse(range).ok()?;

    // The re-serialized canonical form normalizes whitespace and
    // comparator order for free; the explicit caret/tilde/partial
    // forms below capture the reductions Display doesn't perform.
    let mut candidates = vec![parsed.to_string()];
    if let Some(min) = parsed.min_version()
        && !min.is_prerelease()
    {
        let (major, minor, patch) = (min.major, min.minor, min.patch);
        candidates.push(format!("{major}"));
        candidates.push(format!("{major}.{minor}"));
        candidates.push(format!("{major}.{minor}.{patch}"));
        candidates.push(format!("^{major}.{minor}.{patch}"));
        candidates.push(format!("~{major}.{minor}.{patch}"));
    }

    candidates
        .into_iter()
        .filter(|candidate| candidate.len() < range.len())
        .filter(|candidate| Range::parse(candidate).is_ok_and(|reparsed| reparsed == parsed))
        .min_by_key(String::len)
}

#[cfg(test)]
mod tests;
