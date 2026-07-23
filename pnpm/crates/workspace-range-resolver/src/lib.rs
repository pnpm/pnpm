//! Picks the best workspace-sibling version for one of the `workspace:`
//! range tokens — `*`, `^`, `~`, the empty string, or an arbitrary
//! semver range.

use node_semver::{Range, Version};

/// Pick the highest workspace-sibling version matching `range`.
///
/// `range` is the `<version>` portion of a `workspace:` specifier (see
/// `pacquet-workspace-spec`'s `WorkspaceSpec`). The four sentinel tokens
/// (`*`, `^`, `~`, `""`) widen the search to *all* versions, prereleases
/// included. Any other input is treated as a node-semver range and
/// prereleases are excluded unless the range itself carries a
/// prerelease tag.
///
/// Returns the matching raw version string (one of the entries in
/// `versions`) or `None` when nothing satisfies.
#[must_use]
pub fn resolve_workspace_range(range: &str, versions: &[String]) -> Option<String> {
    if is_wildcard(range) {
        return max_version_including_prerelease(versions);
    }
    max_satisfying(versions, range)
}

fn is_wildcard(range: &str) -> bool {
    matches!(range, "*" | "^" | "~" | "")
}

/// Highest version overall, including prereleases. Since `*` matches
/// everything when prereleases are allowed, this collapses to a direct
/// max over all parseable versions.
fn max_version_including_prerelease(versions: &[String]) -> Option<String> {
    let mut best: Option<(Version, &str)> = None;
    for raw in versions {
        let Ok(parsed) = Version::parse(raw) else { continue };
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, raw.as_str())),
        }
    }
    best.map(|(_, raw)| raw.to_string())
}

/// Highest version satisfying `range`. Prereleases are excluded unless
/// the range itself contains a prerelease tag.
fn max_satisfying(versions: &[String], range: &str) -> Option<String> {
    let parsed_range = Range::parse(range).ok()?;
    let range_allows_prereleases = range_allows_prereleases(range);
    let mut best: Option<(Version, &str)> = None;
    for raw in versions {
        let Ok(parsed) = Version::parse(raw) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        if !parsed.pre_release.is_empty() && !range_allows_prereleases {
            continue;
        }
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, raw.as_str())),
        }
    }
    best.map(|(_, raw)| raw.to_string())
}

/// Heuristic for whether `range` would match prereleases without the
/// `includePrerelease` flag.
///
/// In node-semver a range only matches prereleases when one of its
/// comparators carries a `-<pre>` tag (e.g. `>=1.2.3-rc.0` matches
/// `1.2.3-rc.1` but not `1.2.4-rc.0`). The exact rule is involved, but
/// for our purposes "the range string contains a `-` after a digit"
/// is a tight enough approximation.
fn range_allows_prereleases(range: &str) -> bool {
    let bytes = range.as_bytes();
    let mut prev_is_digit = false;
    for &byte in bytes {
        if byte == b'-' && prev_is_digit {
            return true;
        }
        prev_is_digit = byte.is_ascii_digit();
    }
    false
}
