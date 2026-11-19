use miette::Diagnostic;
use pnpm_network::redact_and_sanitize;
use pnpm_resolving_resolver_base::{NoMatchingVersionError, WantedDependency};
use pnpm_semver_include_prerelease::{is_valid_semver_range, parse_strict_semver_range};

use super::ResolveDependencyTreeError;

/// An `overrides` entry, reduced to what attributing a missing version to it
/// needs.
#[derive(Debug, Clone)]
pub struct OverrideEntry {
    /// The entry's key as the user wrote it, e.g. `foo@^1`.
    pub selector: String,
    /// The package the entry targets.
    pub name: String,
    /// The version range of the target in the selector, if it has one.
    pub selector_range: Option<String>,
    /// The specifier the entry forces onto the target.
    pub new_bare_specifier: String,
}

/// `ERR_PNPM_NO_MATCHING_VERSION` for a specifier that an override forced
/// onto a dependency. It names the override entry to fix instead of the
/// package that happened to depend on the target.
#[derive(Debug)]
pub struct OverrideNoMatchingVersionError {
    pub name: String,
    pub selector: String,
    pub new_bare_specifier: String,
    pub selector_range: Option<String>,
    /// The highest published version satisfying `selector_range`.
    pub latest_matching: Option<String>,
}

impl std::fmt::Display for OverrideNoMatchingVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"Override "{}": "{}" targets a version of {} that does not exist on the registry."#,
            redact_and_sanitize(&self.selector),
            redact_and_sanitize(&self.new_bare_specifier),
            redact_and_sanitize(&self.name),
        )
    }
}

impl std::error::Error for OverrideNoMatchingVersionError {}

impl Diagnostic for OverrideNoMatchingVersionError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("ERR_PNPM_NO_MATCHING_VERSION"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let range = self.selector_range.as_deref()?;
        let latest = self.latest_matching.as_deref()?;
        Some(Box::new(format!(
            r#"The latest release of {} matching "{}" is "{latest}"."#,
            redact_and_sanitize(&self.name),
            redact_and_sanitize(range),
        )))
    }
}

/// Attribute a [`ResolveDependencyTreeError::NoMatchingVersion`] to the
/// override whose `new_bare_specifier` the failing request carries. Every
/// other error, and a failure no override explains, is returned unchanged.
pub(super) fn attribute_to_override(
    err: ResolveDependencyTreeError,
    wanted: &WantedDependency,
    overrides: Option<&[OverrideEntry]>,
) -> ResolveDependencyTreeError {
    let reframed = match (&err, overrides) {
        (ResolveDependencyTreeError::NoMatchingVersion(no_matching_version), Some(overrides)) => {
            overriding_error(wanted, no_matching_version, overrides)
        }
        _ => None,
    };
    match reframed {
        Some(reframed) => ResolveDependencyTreeError::OverrideNoMatchingVersion(Box::new(reframed)),
        None => err,
    }
}

fn overriding_error(
    wanted: &WantedDependency,
    no_matching_version: &NoMatchingVersionError,
    overrides: &[OverrideEntry],
) -> Option<OverrideNoMatchingVersionError> {
    let alias = wanted.alias.as_deref()?;
    let bare_specifier = wanted.bare_specifier.as_deref()?;
    let matched = overrides
        .iter()
        .find(|entry| entry.name == alias && entry.new_bare_specifier == bare_specifier)?;
    let selector_range = matched.selector_range.clone();
    // The registry metadata belongs to the package the forced specifier
    // names, so it describes the target only for a plain range, not for an
    // alias such as `npm:other@1`.
    let latest_matching = selector_range
        .as_deref()
        .filter(|_| is_valid_semver_range(&matched.new_bare_specifier))
        .and_then(|range| max_satisfying(&no_matching_version.versions, range));
    Some(OverrideNoMatchingVersionError {
        name: alias.to_string(),
        selector: matched.selector.clone(),
        new_bare_specifier: matched.new_bare_specifier.clone(),
        selector_range,
        latest_matching,
    })
}

pub(super) fn max_satisfying(versions: &[String], range: &str) -> Option<String> {
    let range = parse_strict_semver_range(range)?;
    versions
        .iter()
        .filter_map(|raw| Some((node_semver::Version::parse(raw).ok()?, raw)))
        .filter(|(version, _)| version.satisfies(&range))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, raw)| raw.clone())
}
