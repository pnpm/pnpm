//! Checks a package's wanted `engines` against the current runtime.

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pnpm_semver_include_prerelease::IncludePrereleaseRange;
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
            Ok(version) => IncludePrereleaseRange::parse(wanted_pnpm).satisfies(&version),
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
    Ok(IncludePrereleaseRange::parse(wanted).satisfies(&version))
}
