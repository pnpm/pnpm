use miette::Diagnostic;
use pnpm_resolving_resolver_base::{
    GitResolveError, NoMatchingVersionError, RegistryResponseError, ResolveError, WantedDependency,
};

use super::ResolveDependencyTreeError;

/// Diagnostic for `ERR_PNPM_NO_MATCHING_VERSION` when forced by an override entry.
#[derive(Debug)]
pub struct OverrideNoMatchingVersionError {
    pub name: String,
    pub selector: String,
    pub new_bare_specifier: String,
    pub selector_range: Option<String>,
    pub best: Option<String>,
}

impl std::fmt::Display for OverrideNoMatchingVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"Override "{}": "{}" targets a version of {} that does not exist on the registry."#,
            self.selector, self.new_bare_specifier, self.name,
        )
    }
}

impl std::error::Error for OverrideNoMatchingVersionError {}

impl Diagnostic for OverrideNoMatchingVersionError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("ERR_PNPM_NO_MATCHING_VERSION"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        match (&self.selector_range, &self.best) {
            (Some(range), Some(best)) => Some(Box::new(format!(
                r#"The latest release of {} matching "{}" is "{}"."#,
                self.name, range, best,
            ))),
            _ => None,
        }
    }
}

pub(crate) fn reframe_override_no_matching_version(
    wanted: &WantedDependency,
    specific: &NoMatchingVersionError,
    overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
) -> Option<ResolveDependencyTreeError> {
    let overrides = overrides?;
    let alias = wanted.alias.as_deref()?;
    let bare_specifier = wanted.bare_specifier.as_deref()?;
    let matched = overrides
        .iter()
        .find(|override_entry| {
            override_entry.target_pkg.name == alias
                && override_entry.new_bare_specifier == bare_specifier
        })?;
    let selector_range = matched.target_pkg.bare_specifier.clone();
    let best =
        selector_range.as_deref().and_then(|range| max_satisfying(&specific.versions, range));
    Some(ResolveDependencyTreeError::OverrideNoMatchingVersion(Box::new(
        OverrideNoMatchingVersionError {
            name: alias.to_string(),
            selector: matched.selector.clone(),
            new_bare_specifier: matched.new_bare_specifier.clone(),
            selector_range,
            best,
        },
    )))
}

pub(crate) fn max_satisfying(versions: &[String], range: &str) -> Option<String> {
    let parsed_range = node_semver::Range::parse(range).ok()?;
    let mut best: Option<(node_semver::Version, String)> = None;
    for version in versions {
        let Ok(parsed) = node_semver::Version::parse(version) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, version.clone())),
        }
    }
    best.map(|(_, raw)| raw)
}

/// Wrap a resolver-chain failure, keeping the pnpm error code of the ones
/// that carry one.
pub(crate) fn map_resolve_error(
    err: ResolveError,
    wanted: &WantedDependency,
    overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
) -> ResolveDependencyTreeError {
    let err = match err.downcast::<NoMatchingVersionError>() {
        Ok(no_matching_version) => {
            return match reframe_override_no_matching_version(
                wanted,
                &no_matching_version,
                overrides,
            ) {
                Some(reframed) => reframed,
                None => ResolveDependencyTreeError::NoMatchingVersion(*no_matching_version),
            };
        }
        Err(err) => err,
    };
    let err = match err.downcast::<RegistryResponseError>() {
        Ok(response) => return ResolveDependencyTreeError::RegistryResponse(*response),
        Err(err) => err,
    };
    match err.downcast::<GitResolveError>() {
        Ok(git) => ResolveDependencyTreeError::GitResolve(*git),
        Err(err) => ResolveDependencyTreeError::Resolve(err.to_string()),
    }
}
