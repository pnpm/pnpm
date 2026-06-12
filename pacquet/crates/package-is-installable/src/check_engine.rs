//! Port of `checkEngine.ts` from
//! <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkEngine.ts>.

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
        parts.push(format!("\"node\":{n:?}"));
    }
    if let Some(p) = pnpm {
        parts.push(format!("\"pnpm\":{p:?}"));
    }
    format!("{{{}}}", parts.join(","))
}

/// Thrown when the configured `nodeVersion` is not a valid exact
/// semver version. Mirrors pnpm's `ERR_PNPM_INVALID_NODE_VERSION` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkEngine.ts#L25-L27>.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("The nodeVersion setting is \"{node_version}\", which is not exact semver version")]
#[diagnostic(code(ERR_PNPM_INVALID_NODE_VERSION))]
pub struct InvalidNodeVersionError {
    pub node_version: String,
}

/// Evaluate a wanted `engines` block against the current engine.
///
/// Returns:
/// - `Ok(None)`: the runtime satisfies every declared range.
/// - `Ok(Some(UnsupportedEngineError))`: at least one declared range
///   was not satisfied; the error lists only the unsatisfied entries
///   in its `wanted` field.
/// - `Err(InvalidNodeVersionError)`: the supplied `current.node` is
///   not a parseable exact semver (e.g. user passed a range like
///   `>=20.0.0` into the `nodeVersion` config).
///
/// The semver `satisfies` call uses `includePrerelease: true` upstream
/// (so a `21.0.0-nightly...` host satisfies `^14.18.0 || >=16.0.0`).
/// `node-semver`'s Rust port doesn't expose that flag, so this port
/// approximates the behavior — see `satisfies_with_prerelease` in
/// this module for the strategy + known divergences.
pub fn check_engine(
    package_id: &str,
    wanted: &WantedEngine,
    current: &Engine,
) -> Result<Option<UnsupportedEngineError>, InvalidNodeVersionError> {
    let mut unsatisfied = WantedEngine::default();

    if let Some(wanted_node) = wanted.node.as_ref() {
        match node_satisfies(&current.node, wanted_node) {
            // Range matched — nothing to do.
            Ok(true) => {}
            // `current.node` is a valid version but doesn't satisfy
            // the range — record the unsatisfied wanted entry.
            // `node_satisfies` already parsed it once, so we don't
            // re-parse here.
            Ok(false) => unsatisfied.node = Some(wanted_node.clone()),
            // `current.node` is not a parseable exact semver — this
            // is the `ERR_PNPM_INVALID_NODE_VERSION` path upstream
            // throws from inside `checkEngine`.
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

/// Approximation of npm-semver's `satisfies(version, range, { includePrerelease: true })`.
///
/// `node-semver` (the Rust crate) doesn't expose `includePrerelease`;
/// its `Range::satisfies` enforces strict semver prerelease compat,
/// where a prerelease version only matches a range with an explicit
/// prerelease bound at the same major.minor.patch. Upstream pnpm
/// always wants prereleases to count for engine checks, so a
/// `21.0.0-nightly...` host should satisfy `>=16.0.0`.
///
/// Strategy:
/// 1. Try the strict check first. Most callers ship release versions
///    and this is the correct, byte-for-byte path.
/// 2. If the strict check fails AND the version is a prerelease,
///    re-check with the prerelease stripped (i.e. `21.0.0-nightly`
///    becomes `21.0.0`). This permits a prerelease to satisfy any
///    range its semantically-equivalent release would.
///
/// The fallback is a controlled over-acceptance: a prerelease
/// `X.Y.Z-rc1` is taken as `X.Y.Z` for the bounded comparison. The
/// realistic engine ranges that use just a major (`>=N`, `^N`, plain
/// `N`) all behave correctly because `node-semver` expands those to
/// a partial-version range that already accepts prereleases of N at
/// the lower bound.
///
/// Two known divergences from upstream's `includePrerelease: true`:
///
/// - `>=X.Y.Z` (strict lower bound, fully specified mmp): upstream
///   rejects `X.Y.Z-rc1` because alpha-class prereleases sort below
///   the corresponding release in semver, so `X.Y.Z-rc1 < X.Y.Z`.
///   Pacquet's strip turns the version into `X.Y.Z` which then
///   satisfies `>=X.Y.Z` — over-acceptance. Pinned by the integration
///   test `pnpm_is_a_prerelease_version_strict_ge_full_version_does_not_satisfy`
///   under `known_failures`.
/// - `<X.Y.Z` (strict upper bound, fully specified mmp): upstream
///   accepts `X.Y.Z-rc1` (semver-less-than); pacquet's strip turns
///   it into `X.Y.Z` which is NOT `<X.Y.Z` — under-acceptance.
///
/// Engine ranges with either of those exact shapes are vanishingly
/// rare in real `package.json` files. A future change that lands
/// byte-for-byte `includePrerelease: true` semantics (the
/// `nodejs-semver` fork, or an open-coded bound walk) closes both
/// gaps.
fn satisfies_with_prerelease(version: &Version, wanted_range: &str) -> bool {
    let Ok(range) = Range::parse(wanted_range) else {
        // Match upstream `semver.satisfies` returning `false` for
        // an unparsable range rather than throwing.
        return false;
    };
    if range.satisfies(version) {
        return true;
    }
    if !version.pre_release.is_empty() {
        let stripped = Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            pre_release: Vec::new(),
            build: version.build.clone(),
        };
        return range.satisfies(&stripped);
    }
    false
}
