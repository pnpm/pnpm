//! [Package URLs](https://github.com/package-url/purl-spec) for the
//! packages pnpr serves, built as the purl specification's "How to build a
//! PURL" describes, with each type's normalization.

use derive_more::{Display, Error};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::fmt;

/// What a PURL component leaves unencoded: alphanumerics, the punctuation
/// `.-_~`, and the colon.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// A Package URL, such as `pkg:npm/%40example/widgets@1.2.0`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Purl(String);

/// Why a Package URL could not be built.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PurlError {
    #[display("a package URL needs a name")]
    EmptyName,
    #[display("a package URL of a release needs a version")]
    EmptyVersion,
    #[display("npm package name has an empty scope or name: {name}")]
    InvalidNpmName {
        #[error(not(source))]
        name: String,
    },
}

impl Purl {
    /// The PURL of an npm package version, with the scope as its namespace:
    /// `@example/widgets` at `1.2.0` is `pkg:npm/%40example/widgets@1.2.0`.
    pub fn npm(name: &str, version: &str) -> Result<Self, PurlError> {
        let (namespace, bare_name) = match name.strip_prefix('@') {
            Some(scoped) => {
                let (scope, bare_name) = scoped
                    .split_once('/')
                    .unwrap_or(("", ""));
                if scope.is_empty() || bare_name.is_empty() {
                    return Err(PurlError::InvalidNpmName { name: name.to_owned() });
                }
                (Some(&name[..=scope.len()]), bare_name)
            }
            None => (None, name),
        };
        Purl::build("npm", namespace, bare_name, version, &[])
    }

    /// The PURL of a crate version, such as `pkg:cargo/serde@1.0.210`.
    pub fn cargo(name: &str, version: &str) -> Result<Self, PurlError> {
        Purl::build("cargo", None, name, version, &[])
    }

    /// The PURL of one distribution file of a Python release, named by the
    /// `file_name` qualifier. The project name is lowercased with `_`
    /// replaced by `-`, as the pypi type requires.
    pub fn pypi_file(name: &str, version: &str, file_name: &str) -> Result<Self, PurlError> {
        let name = name.to_lowercase().replace('_', "-");
        Purl::build("pypi", None, &name, version, &[("file_name", file_name)])
    }

    /// The PURL of an OCI image a tag points at: the last segment of the
    /// repository, lowercased, as the name, and the manifest digest as the
    /// version, with the repository URL and the tag as qualifiers.
    pub fn oci_tag(
        repository_url: &str,
        manifest_digest: &str,
        tag: &str,
    ) -> Result<Self, PurlError> {
        let name = repository_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let qualifiers = [("repository_url", repository_url), ("tag", tag)];
        Purl::build("oci", None, &name.to_lowercase(), &manifest_digest.to_lowercase(), &qualifiers)
    }

    /// The PURL's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The PURL type, such as `npm`.
    #[must_use]
    pub fn purl_type(&self) -> &str {
        let rest = self.0.strip_prefix("pkg:").unwrap_or_default();
        rest.split_once('/').map_or(rest, |(purl_type, _)| purl_type)
    }

    /// Wraps an existing PURL, such as one read from a record, without
    /// normalizing it.
    #[must_use]
    pub fn from_string_unchecked(text: String) -> Self {
        Purl(text)
    }

    fn build(
        purl_type: &str,
        namespace: Option<&str>,
        name: &str,
        version: &str,
        qualifiers: &[(&str, &str)],
    ) -> Result<Self, PurlError> {
        if name.is_empty() {
            return Err(PurlError::EmptyName);
        }
        if version.is_empty() {
            return Err(PurlError::EmptyVersion);
        }
        let namespace = namespace
            .map(|namespace| format!("{}/", encode(namespace)))
            .unwrap_or_default();
        let (name, version) = (encode(name), encode(version));
        let mut pairs: Vec<String> = qualifiers
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(key, value)| format!("{key}={}", encode(value)))
            .collect();
        pairs.sort_unstable();
        let qualifiers =
            if pairs.is_empty() { String::new() } else { format!("?{}", pairs.join("&")) };
        Ok(Purl(format!("pkg:{purl_type}/{namespace}{name}@{version}{qualifiers}")))
    }
}

impl fmt::Display for Purl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn encode(component: &str) -> String {
    utf8_percent_encode(component, COMPONENT).to_string()
}

#[cfg(test)]
mod tests;
