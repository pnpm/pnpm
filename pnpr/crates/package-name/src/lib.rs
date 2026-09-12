use derive_more::{Display, Error};
use pep508_rs::PackageName as PythonPackageName;
use pnpr_error::RegistryError;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use strum::{EnumIter, IntoEnumIterator as _};

/// The protocol whose naming rules a registry surface follows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    #[default]
    Npm,
    Cargo,
    Pypi,
    Oci,
}

impl Ecosystem {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Cargo => "cargo",
            Ecosystem::Pypi => "pypi",
            Ecosystem::Oci => "oci",
        }
    }

    /// Every ecosystem pnpr can serve.
    ///
    /// Derived rather than written out, because a caller asking "is this the
    /// only one configured" has to compare against all of them, and a list
    /// maintained by hand answers that wrongly the moment an ecosystem is
    /// added without it.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::iter()
    }
}

impl fmt::Display for Ecosystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub const MAX_CRATE_NAME_LEN: usize = 64;

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum CrateNameError {
    #[display("crate name must not be empty")]
    Empty,
    #[display("crate name {name:?} is longer than {MAX_CRATE_NAME_LEN} characters")]
    TooLong { name: String },
    #[display("crate name {name:?} must start with a letter or `_`")]
    InvalidStart { name: String },
    #[display("crate name {name:?} may only contain ASCII letters, digits, `-` and `_`")]
    InvalidCharacter { name: String },
}

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("{name:?} is not a valid Python project name")]
pub struct PythonNameError {
    pub name: String,
}

/// The OCI distribution spec's limit on a whole repository name.
pub const MAX_OCI_NAME_LEN: usize = 255;

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum OciNameError {
    #[display("image name must not be empty")]
    Empty,
    #[display("image name {name:?} is longer than {MAX_OCI_NAME_LEN} characters")]
    TooLong { name: String },
    #[display("image name {name:?} has an empty path component")]
    EmptyComponent { name: String },
    #[display("image name component {component:?} must start and end with a letter or digit")]
    ComponentBoundary { component: String },
    #[display(
        "image name component {component:?} may only contain letters, digits, `.`, `_` and `-`"
    )]
    InvalidCharacter { component: String },
    #[display(
        "image name component {component:?} may only join alphanumeric runs with `.`, `_`, `__`, or `-`"
    )]
    ComponentSeparator { component: String },
}

/// The canonical name of an npm package, Cargo crate, Python project, or OCI
/// image repository.
/// Construction applies the ecosystem's normalization and validation before
/// the name can be used as a storage or cache key.
#[derive(Debug, Clone)]
pub struct CanonicalPackageName {
    raw: String,
    /// The unscoped portion — for `@scope/name` and `namespace/image` this is
    /// the last component, for `name` this is the whole thing. Used to
    /// validate tarball filenames, which are always `<basename>-<version>.tgz`.
    basename: String,
}

impl CanonicalPackageName {
    pub fn parse(raw: &str, ecosystem: Ecosystem) -> Result<Self, RegistryError> {
        let canonical = match ecosystem {
            Ecosystem::Npm => raw.to_string(),
            Ecosystem::Cargo => canonicalize_crate_name(raw)
                .map_err(|error| invalid_ecosystem_name(raw, ecosystem, error.to_string()))?,
            Ecosystem::Pypi => canonicalize_python_name(raw)
                .map_err(|error| invalid_ecosystem_name(raw, ecosystem, error.to_string()))?,
            // An image name is many `/`-joined components and may outrun the
            // npm length limit, so the OCI grammar is the whole check: it
            // holds every component to starting and ending alphanumeric,
            // which is stricter than `is_safe_segment` asks for.
            Ecosystem::Oci => {
                let canonical = canonicalize_oci_name(raw)
                    .map_err(|error| invalid_ecosystem_name(raw, ecosystem, error.to_string()))?;
                let basename =
                    canonical.rsplit_once('/').map_or(canonical.as_str(), |(_, last)| last);
                let basename = basename.to_string();
                return Ok(Self { raw: canonical, basename });
            }
        };
        Self::parse_canonical(&canonical).map_err(|error| match ecosystem {
            Ecosystem::Npm => error,
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => invalid_ecosystem_name(
                raw,
                ecosystem,
                "its canonical form is not a safe registry key".to_string(),
            ),
        })
    }

    fn parse_canonical(raw: &str) -> Result<Self, RegistryError> {
        let invalid = || RegistryError::InvalidPackageName { name: raw.to_string() };
        if raw.is_empty() || raw.len() > 214 {
            return Err(invalid());
        }
        let basename = if let Some(rest) = raw.strip_prefix('@') {
            let (scope, name) = rest.split_once('/').ok_or_else(invalid)?;
            if !is_safe_segment(scope) || !is_safe_segment(name) {
                return Err(invalid());
            }
            name.to_string()
        } else {
            if !is_safe_segment(raw) {
                return Err(invalid());
            }
            raw.to_string()
        };
        Ok(Self { raw: raw.to_string(), basename })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    #[must_use]
    pub fn tarball_name_for_version(&self, version: &str) -> String {
        format!("{}-{version}.tgz", self.basename)
    }

    /// Validate `filename` and return the canonical disk filename
    /// (`<basename>-<version>.tgz`). Used by the publish handler so
    /// libnpmpublish's `@scope/name-1.0.0.tgz` attachment lands on
    /// disk under the same path the GET endpoint serves.
    pub fn canonicalize_tarball_name(&self, filename: &str) -> Result<String, RegistryError> {
        self.parse_tarball_name(filename).map(|(canonical, _)| canonical)
    }

    /// Like [`Self::canonicalize_tarball_name`] but also returns the
    /// version segment extracted from the filename. The publish
    /// handler uses the version to look up `versions[v].dist` and
    /// verify the tarball's integrity against what the packument
    /// declares.
    pub fn parse_tarball_name(&self, filename: &str) -> Result<(String, String), RegistryError> {
        let invalid = || RegistryError::InvalidTarballName {
            package: self.raw.clone(),
            filename: filename.to_string(),
        };
        let stem = filename.strip_suffix(".tgz").ok_or_else(invalid)?;
        // Try the longer prefix first so that for an unscoped package
        // (where `self.raw == self.basename`) we still match.
        let rest = stem
            .strip_prefix(&self.raw)
            .or_else(|| stem.strip_prefix(&self.basename))
            .ok_or_else(invalid)?;
        let version = rest.strip_prefix('-').ok_or_else(invalid)?;
        if !is_safe_segment(version) {
            return Err(invalid());
        }
        Ok((self.tarball_name_for_version(version), version.to_string()))
    }
}

fn invalid_ecosystem_name(name: &str, ecosystem: Ecosystem, reason: String) -> RegistryError {
    RegistryError::InvalidEcosystemPackageName {
        name: name.to_string(),
        ecosystem: ecosystem.to_string(),
        reason,
    }
}

pub fn canonicalize_crate_name(name: &str) -> Result<String, CrateNameError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(CrateNameError::Empty);
    };
    if name.len() > MAX_CRATE_NAME_LEN {
        return Err(CrateNameError::TooLong { name: name.to_string() });
    }
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(CrateNameError::InvalidStart { name: name.to_string() });
    }
    if !chars.all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')) {
        return Err(CrateNameError::InvalidCharacter { name: name.to_string() });
    }
    Ok(name.to_ascii_lowercase())
}

pub fn canonicalize_python_name(raw: &str) -> Result<String, PythonNameError> {
    let invalid = || PythonNameError { name: raw.to_string() };
    let normalized = PythonPackageName::from_str(raw).map_err(|_| invalid())?.as_ref().to_string();
    let well_formed =
        normalized.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        }) && normalized.chars().next().is_some_and(|character| character.is_ascii_alphanumeric())
            && normalized
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric());
    well_formed.then_some(normalized).ok_or_else(invalid)
}

/// Normalize and validate an OCI repository name against the distribution
/// spec's grammar: `/`-joined components, each a `.`, `_`, `__`, or `-`-joined
/// run of alphanumerics. Uppercase is folded rather than refused, so two names
/// that differ only in case cannot become two repositories on a
/// case-insensitive filesystem.
pub fn canonicalize_oci_name(raw: &str) -> Result<String, OciNameError> {
    if raw.is_empty() {
        return Err(OciNameError::Empty);
    }
    if raw.len() > MAX_OCI_NAME_LEN {
        return Err(OciNameError::TooLong { name: raw.to_string() });
    }
    let canonical = raw.to_ascii_lowercase();
    for component in canonical.split('/') {
        validate_oci_component(component, &canonical)?;
    }
    Ok(canonical)
}

fn validate_oci_component(component: &str, name: &str) -> Result<(), OciNameError> {
    if component.is_empty() {
        return Err(OciNameError::EmptyComponent { name: name.to_string() });
    }
    let alphanumeric = |character: char| character.is_ascii_alphanumeric();
    if !component.starts_with(alphanumeric) || !component.ends_with(alphanumeric) {
        return Err(OciNameError::ComponentBoundary { component: component.to_string() });
    }
    if component.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    }) {
        return Err(OciNameError::InvalidCharacter { component: component.to_string() });
    }
    let mut rest = component;
    while let Some(start) = rest.find(|character: char| !character.is_ascii_alphanumeric()) {
        let tail = &rest[start..];
        let end = tail.find(alphanumeric).unwrap_or(tail.len());
        if !is_oci_separator(&tail[..end]) {
            return Err(OciNameError::ComponentSeparator { component: component.to_string() });
        }
        rest = &tail[end..];
    }
    Ok(())
}

fn is_oci_separator(run: &str) -> bool {
    matches!(run, "." | "_" | "__") || run.bytes().all(|byte| byte == b'-')
}

// `:` is rejected because on Windows `C:foo` is a drive-relative *prefix*
// component: `PathBuf::join` treats it as a new path rather than a child
// segment, so a `:`-carrying name or filename could escape the storage or
// cache root.
//
// `?`, `#`, `%`, whitespace, and control characters are rejected because a name
// reaches an upstream by interpolation into its URL: the request path is
// percent-decoded before it gets here, so `foo#bar` and `foo?bar` fetch `foo`
// while being authorized and cached under a name of their own.
//
// No package name, semver version, or artifact filename in any ecosystem this
// serves carries one of these.
fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && segment != ".."
        && !segment.chars().any(|character| {
            matches!(character, '/' | '\\' | ':' | '?' | '#' | '%')
                || character.is_whitespace()
                || character.is_control()
        })
}

/// Whether `filename` is safe to use as a single on-disk path segment (no
/// traversal, no separators, no absolute-path or Windows drive prefixes). The
/// upstream tarball path uses it to admit a non-canonical basename preserved
/// from an upstream `dist.tarball` (see `rewrite_tarball_urls`) into the
/// cache layout — the packument match is what authorizes the name; this only
/// keeps it on disk safely.
#[must_use]
pub fn is_safe_path_segment(filename: &str) -> bool {
    is_safe_segment(filename)
}

#[cfg(test)]
mod tests;
