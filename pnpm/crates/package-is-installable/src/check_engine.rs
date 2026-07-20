//! Checks a package's wanted `engines` against the current runtime.

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use serde::Serialize;

/// Wanted engine versions declared by a package's `engines` field.
///
/// Both members are optional and carry npm-style range strings.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct WantedEngine {
    pub node: Option<String>,
    pub pnpm: Option<String>,
}

/// Current runtime engine versions. `node` is mandatory (no install
/// without a node version on PATH or in config), `pnpm` is optional
/// — pacquet itself is not pnpm, so callers normally pass `None`
/// here, which matches upstream's behavior of skipping the pnpm
/// check entirely when `currentEngine.pnpm` is unset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Engine {
    pub node: String,
    pub pnpm: Option<String>,
}

/// Error returned by [`check_engine`] when the runtime fails to
/// satisfy a wanted range. Wire-compatible with pnpm's
/// `ERR_PNPM_UNSUPPORTED_ENGINE` (same code, same message shape).
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("Unsupported engine for {package_id}: wanted: {wanted_json} (current: {current_json})")]
#[diagnostic(code(ERR_PNPM_UNSUPPORTED_ENGINE))]
pub struct UnsupportedEngineError {
    pub package_id: String,
    pub wanted: WantedEngine,
    pub current: Engine,
    wanted_json: String,
    current_json: String,
}

impl UnsupportedEngineError {
    fn new(package_id: String, wanted: WantedEngine, current: Engine) -> Self {
        let wanted_json = engine_json(wanted.node.as_deref(), wanted.pnpm.as_deref());
        let current_json = engine_json(Some(current.node.as_str()), current.pnpm.as_deref());
        Self { package_id, wanted, current, wanted_json, current_json }
    }
}

fn engine_json(node: Option<&str>, pnpm: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(n) = node {
        parts.push(format!(r#""node":{n:?}"#));
    }
    if let Some(p) = pnpm {
        parts.push(format!(r#""pnpm":{p:?}"#));
    }
    format!("{{{}}}", parts.join(","))
}

/// Thrown when the configured `nodeVersion` is not a valid exact
/// semver version. Carries the `ERR_PNPM_INVALID_NODE_VERSION` code.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("The nodeVersion setting is \"{node_version}\", which is not exact semver version")]
#[diagnostic(code(ERR_PNPM_INVALID_NODE_VERSION))]
pub struct InvalidNodeVersionError {
    pub node_version: String,
}

/// Evaluate a wanted `engines` block against the current engine.
///
/// The error lists only the unsatisfied entries in its `wanted` field.
///
/// The semver `satisfies` call uses `includePrerelease: true` upstream
/// (so a `21.0.0-nightly...` host satisfies `^14.18.0 || >=16.0.0`).
/// `node-semver`'s Rust port doesn't expose that flag, so this port
/// reimplements it — see `satisfies_with_prerelease` in this module
/// for the strategy + the one remaining divergence.
pub fn check_engine(
    package_id: &str,
    wanted: &WantedEngine,
    current: &Engine,
) -> Result<Option<UnsupportedEngineError>, InvalidNodeVersionError> {
    let mut unsatisfied = WantedEngine::default();

    if let Some(wanted_node) = wanted.node.as_ref() {
        match node_satisfies(&current.node, wanted_node) {
            Ok(true) => {}
            Ok(false) => unsatisfied.node = Some(wanted_node.clone()),
            Err(InvalidVersion) => {
                return Err(InvalidNodeVersionError { node_version: current.node.clone() });
            }
        }
    }

    if let (Some(current_pnpm), Some(wanted_pnpm)) = (current.pnpm.as_ref(), wanted.pnpm.as_ref()) {
        let satisfied = match Version::parse(current_pnpm) {
            Ok(version) => satisfies_with_prerelease(&version, wanted_pnpm),
            Err(_) => false,
        };
        if !satisfied {
            unsatisfied.pnpm = Some(wanted_pnpm.clone());
        }
    }

    if unsatisfied.node.is_some() || unsatisfied.pnpm.is_some() {
        return Ok(Some(UnsupportedEngineError::new(
            package_id.to_string(),
            unsatisfied,
            current.clone(),
        )));
    }
    Ok(None)
}

struct InvalidVersion;

fn node_satisfies(current: &str, wanted: &str) -> Result<bool, InvalidVersion> {
    let version = Version::parse(current).map_err(|_| InvalidVersion)?;
    Ok(satisfies_with_prerelease(&version, wanted))
}

/// Rust port of npm-semver's `satisfies(version, range, { includePrerelease: true })`.
///
/// `node-semver` (the Rust crate) doesn't expose `includePrerelease`;
/// its `Range::satisfies` enforces strict semver prerelease compat,
/// where a prerelease version only matches a range with an explicit
/// prerelease bound at the same major.minor.patch. Upstream pnpm
/// always wants prereleases to count for engine checks, so a
/// `21.0.0-nightly...` host should satisfy `>=16.0.0`.
///
/// With `includePrerelease: true` npm-semver's behavior splits by how
/// each comparator was written:
///
/// - Comparators with a fully specified version — `>=9.0.0`, `<9.0.0`,
///   a bare `9.0.0` — keep that exact bound and compare by pure semver
///   ordering, so `9.0.0-alpha.1` does *not* satisfy `>=9.0.0`
///   (`alpha.1 < 9.0.0`), while it does satisfy `<9.0.0`.
/// - Everything that npm expands (`9`, `>=9`, `9.x`, `^9.0.0`,
///   `~9.0.0`, hyphen ranges) gets an implicit `-0` floor on its lower
///   bound, so `9.0.0-alpha.1` *does* satisfy `9`, `>=9`, and
///   `^9.0.0`.
///
/// The parsed `Range` can't distinguish the two shapes (`>=9` and
/// `>=9.0.0` parse identically), so for prerelease versions this is
/// evaluated per `||` alternative against the range *string*:
///
/// 1. The strict check runs first — byte-for-byte correct, and the
///    only path release versions ever take.
/// 2. If the alternative contains a comparator that pins prerelease
///    ordering at the version's own base triple (a fully specified
///    primitive or bare exact, or a `^`/`~` spec that itself carries a
///    prerelease), the alternative is decided by pure semver ordering.
/// 3. Otherwise every comparator either has an implicit `-0` floor or
///    its bound sits at a different base triple, where pure ordering
///    and base-version ordering agree — so the check runs with the
///    prerelease stripped (`21.0.0-nightly` becomes `21.0.0`).
///
/// One divergence remains: a conjunction mixing an *expansion* and a
/// *pinning comparator* at the same base triple (e.g.
/// `^9.0.0 >9.0.0-alpha`) drops the expansion's `-0` floor. Ranges of
/// that shape are vanishingly rare in real `package.json` files.
fn satisfies_with_prerelease(version: &Version, wanted_range: &str) -> bool {
    let Ok(range) = Range::parse(wanted_range) else {
        // Match upstream `semver.satisfies` returning `false` for
        // an unparsable range rather than throwing.
        return false;
    };
    if version.pre_release.is_empty() {
        // The strict check only diverges from `includePrerelease: true`
        // on prerelease versions.
        return range.satisfies(version);
    }
    wanted_range
        .split("||")
        .any(|alternative| alternative_satisfies_prerelease(version, alternative.trim()))
}

fn alternative_satisfies_prerelease(version: &Version, alternative: &str) -> bool {
    let Ok(range) = Range::parse(alternative) else {
        return false;
    };
    if range.satisfies(version) {
        return true;
    }
    if has_base_pinning_comparator(alternative, version) {
        return satisfies_by_pure_ordering(version, &range);
    }
    let base = base_version(version);
    range.satisfies(&base)
}

/// Pure semver-ordering satisfaction — the comparator test npm runs
/// once `includePrerelease: true` lifts its prerelease-tuple gate.
/// Encoded as interval overlap between the range and the degenerate
/// `[version, version]` range, which `node-semver` evaluates on raw
/// bound ordering without the gate.
fn satisfies_by_pure_ordering(version: &Version, range: &Range) -> bool {
    let mut exact = version.clone();
    exact.build = Vec::new();
    Range::parse(exact.to_string()).is_ok_and(|exact| range.allows_any(&exact))
}

fn base_version(version: &Version) -> Version {
    Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    }
}

/// Whether `alternative` contains a comparator that pins prerelease
/// ordering at `version`'s base triple: a primitive (`>=`, `>`, `<`,
/// `<=`, `=`) or bare exact with a fully specified version, or a
/// `^`/`~` spec that itself carries a prerelease. npm gives none of
/// these an implicit `-0` floor under `includePrerelease: true`, so
/// when one sits at the version's own base triple the outcome must
/// come from pure semver ordering, not from the stripped-prerelease
/// fallback.
///
/// Hyphen-range endpoints do get the implicit floor, so alternatives
/// containing a hyphen range never pin.
fn has_base_pinning_comparator(alternative: &str, version: &Version) -> bool {
    if alternative.split_whitespace().any(|token| token == "-") {
        return false;
    }
    let mut tokens = alternative.split_whitespace();
    while let Some(token) = tokens.next() {
        // The parser allows spaces between an operator (or `^`/`~`)
        // and its version (`>= 9.0.0`), in which case the version is
        // the next token.
        let (comparator, requires_prerelease) = if let Some(rest) = strip_primitive_operator(token)
        {
            (if rest.is_empty() { tokens.next().unwrap_or("") } else { rest }, false)
        } else if let Some(rest) =
            token.strip_prefix("~>").or_else(|| token.strip_prefix(['~', '^']))
        {
            (if rest.is_empty() { tokens.next().unwrap_or("") } else { rest }, true)
        } else {
            (token, false)
        };
        let Ok(parsed) = Version::parse(comparator.trim_start_matches(['v', 'V'])) else {
            continue;
        };
        if requires_prerelease && parsed.pre_release.is_empty() {
            continue;
        }
        if (parsed.major, parsed.minor, parsed.patch)
            == (version.major, version.minor, version.patch)
        {
            return true;
        }
    }
    false
}

fn strip_primitive_operator(token: &str) -> Option<&str> {
    [">=", "<=", ">", "<", "="].iter().find_map(|operator| token.strip_prefix(operator))
}
