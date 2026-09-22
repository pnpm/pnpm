use super::{
    LoadWorkspaceYamlError,
    redact_registry_url,
};
use pnpr_registry::{
    Ecosystem,
    PackagePattern,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PythonPattern {
    Name(PackagePattern),
    Prefix(String),
}

impl PythonPattern {
    pub(super) fn parse(pattern: &str) -> Result<Self, String> {
        if pattern == "*" {
            return Ok(Self::Name(PackagePattern::All));
        }
        if pattern != "**"
            && let Some(prefix) = pattern.strip_suffix('*')
        {
            // A prefix may end with a separator, but a complete Python name cannot.
            let name = PackagePattern::parse(&format!("{prefix}x"), Ecosystem::Pypi)
                .map_err(|error| error.to_string())?;
            let normalized = name.to_string();
            return Ok(Self::Prefix(
                normalized
                    .strip_suffix('x')
                    .expect("Python name normalization preserves the final ASCII letter")
                    .to_string(),
            ));
        }
        PackagePattern::parse(pattern, Ecosystem::Pypi)
            .map(Self::Name)
            .map_err(|error| error.to_string())
    }

    pub(super) fn matches(&self, name: &str) -> bool {
        match self {
            Self::Name(pattern) => pattern.matches(name),
            Self::Prefix(prefix) => name.starts_with(prefix),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Prefix(a), Self::Prefix(b)) => a.starts_with(b) || b.starts_with(a),
            (Self::Name(PackagePattern::Exact(name)), Self::Prefix(prefix))
            | (Self::Prefix(prefix), Self::Name(PackagePattern::Exact(name))) => {
                name.starts_with(prefix)
            }
            (Self::Name(a), Self::Name(b)) => a.covers(b) || b.covers(a),
            _ => true,
        }
    }

    pub(super) fn is_default(&self) -> bool {
        matches!(self, Self::Name(PackagePattern::All))
    }

    pub(super) fn normalized(&self) -> String {
        match self {
            Self::Name(PackagePattern::All) => "*".to_string(),
            Self::Name(pattern) => pattern.to_string(),
            Self::Prefix(prefix) => format!("{prefix}*"),
        }
    }
}

pub(super) fn patterns(
    index: &EcosystemIndex,
) -> Result<Vec<PythonPattern>, LoadWorkspaceYamlError> {
    let invalid = |reason: String| LoadWorkspaceYamlError::InvalidPythonRegistryPackages {
        registry: redact_registry_url(&index.url),
        reason,
    };
    let Some(packages) = &index.packages else {
        return Ok(vec![PythonPattern::Name(PackagePattern::All)]);
    };
    if packages.is_empty() {
        return Err(invalid("packages must not be empty".to_string()));
    }
    let patterns = packages
        .iter()
        .map(|pattern| PythonPattern::parse(pattern).map_err(&invalid))
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
