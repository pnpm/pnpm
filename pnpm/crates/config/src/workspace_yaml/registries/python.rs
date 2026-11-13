use super::{LoadWorkspaceYamlError, redact_registry_url};
use pnpm_package_name::canonicalize_python_name;

/// One ecosystem index and the Python namespace it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcosystemIndex {
    pub url: String,
    pub packages: Option<Vec<String>>,
}

impl From<String> for EcosystemIndex {
    fn from(url: String) -> Self {
        Self { url, packages: None }
    }
}

/// A validated Python namespace and its authoritative index.
#[derive(Debug)]
pub struct PythonRegistryRoute {
    pub url: String,
    patterns: Vec<PythonPattern>,
}

impl PythonRegistryRoute {
    pub fn from_indexes(indexes: &[EcosystemIndex]) -> Result<Vec<Self>, LoadWorkspaceYamlError> {
        validate_routes(indexes)?;
        indexes
            .iter()
            .map(|index| Ok(Self { url: index.url.clone(), patterns: patterns(index)? }))
            .collect()
    }

    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        self.patterns
            .iter()
            .any(|pattern| pattern.matches(name))
    }

    #[must_use]
    pub fn is_default(&self) -> bool {
        self.patterns.iter().any(PythonPattern::is_default)
    }

    #[must_use]
    pub fn packages(&self) -> Vec<String> {
        let mut packages: Vec<_> = self.patterns
            .iter()
            .map(PythonPattern::normalized)
            .collect();
        packages.sort();
        packages
    }
}

/// One entry of a Python index's `packages` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PythonPattern {
    /// `*` or `**`: every project.
    All,
    /// A normalized project name.
    Exact(String),
    /// The normalized start of a project name, written `<prefix>*`.
    Prefix(String),
}

/// The longest package name a registry stores.
const MAX_NAME_LENGTH: usize = 214;

impl PythonPattern {
    pub(super) fn parse(pattern: &str) -> Result<Self, String> {
        if pattern == "*" || pattern == "**" {
            return Ok(Self::All);
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            // A prefix may end with a separator, but a complete Python name cannot.
            let name = exact_name(&format!("{prefix}x"))?;
            return Ok(Self::Prefix(
                name.strip_suffix('x')
                    .expect("Python name normalization preserves the final ASCII letter")
                    .to_string(),
            ));
        }
        exact_name(pattern).map(Self::Exact)
    }

    pub(super) fn matches(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Exact(exact) => exact == name,
            Self::Prefix(prefix) => name.starts_with(prefix),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => true,
            (Self::Exact(a), Self::Exact(b)) => a == b,
            (Self::Prefix(a), Self::Prefix(b)) => a.starts_with(b) || b.starts_with(a),
            (Self::Exact(name), Self::Prefix(prefix))
            | (Self::Prefix(prefix), Self::Exact(name)) => name.starts_with(prefix),
        }
    }

    pub(super) fn is_default(&self) -> bool {
        matches!(self, Self::All)
    }

    pub(super) fn normalized(&self) -> String {
        match self {
            Self::All => "*".to_string(),
            Self::Exact(name) => name.clone(),
            Self::Prefix(prefix) => format!("{prefix}*"),
        }
    }
}

/// The normalized project name `pattern` spells, or why it spells none.
fn exact_name(pattern: &str) -> Result<String, String> {
    if pattern.is_empty() || pattern.contains('*') {
        return Err(format!(
            "unsupported pypi registry pattern {pattern:?}: use an exact name, nothing narrower, \
             or `**`",
        ));
    }
    canonicalize_python_name(pattern)
        .ok()
        .filter(|name| name.len() <= MAX_NAME_LENGTH)
        .ok_or_else(|| {
            format!(
                "registry pattern {pattern:?} is not a valid package name, so it can never \
                 match; to claim every package in a scope use `@scope/*`",
            )
        })
}

pub(super) fn patterns(
    index: &EcosystemIndex,
) -> Result<Vec<PythonPattern>, LoadWorkspaceYamlError> {
    let invalid = |reason: String| LoadWorkspaceYamlError::InvalidPythonRegistryPackages {
        registry: redact_registry_url(&index.url),
        reason,
    };
    let Some(packages) = &index.packages else {
        return Ok(vec![PythonPattern::All]);
    };
    if packages.is_empty() {
        return Err(invalid("packages must not be empty".to_string()));
    }
    let patterns = packages
        .iter()
        .map(|pattern| PythonPattern::parse(pattern).map_err(invalid))
        .collect::<Result<Vec<_>, _>>()?;
    for (i, pattern) in patterns.iter().enumerate() {
        if patterns[..i]
            .iter()
            .any(|other| pattern.overlaps(other))
        {
            return Err(invalid(format!("overlapping package pattern {:?}", packages[i])));
        }
    }
    Ok(patterns)
}

pub(super) fn validate_routes(indexes: &[EcosystemIndex]) -> Result<(), LoadWorkspaceYamlError> {
    let mut claims: Vec<(PythonPattern, &str)> = Vec::new();
    for index in indexes {
        for pattern in patterns(index)? {
            if let Some((_, registry)) = claims
                .iter()
                .find(|(other, _)| {
                    pattern.is_default() == other.is_default() && pattern.overlaps(other)
                })
            {
                return Err(LoadWorkspaceYamlError::PythonPackageRoutedTwice {
                    pattern: pattern.normalized(),
                    registries: super::quote_and_join(
                        [*registry, index.url.as_str()]
                            .map(redact_registry_url)
                            .iter()
                            .map(String::as_str),
                    ),
                });
            }
            claims.push((pattern, &index.url));
        }
    }
    Ok(())
}
