mod purl;

use miette::Result;
use percent_encoding::percent_decode_str;
use purl::{Purl, PurlType};

const CARGO_PROTOCOL: &str = "crate:";
const PYTHON_PROTOCOL: &str = "pypi:";

/// A package request routed to a non-Node.js ecosystem.
///
/// New language protocols belong here. The npm add path only receives the
/// selectors left in [`PackageSpecifierPlan::node_packages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EcosystemPackageSpecifier {
    Cargo(RegistryPackageSpecifier),
    Python(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryPackageSpecifier {
    pub(crate) name: String,
    pub(crate) version_spec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageSpecifierPlan {
    pub(crate) node_packages: Vec<String>,
    /// The members of [`Self::node_packages`] a Package URL was rewritten
    /// into. A purl names a package in a registry, so such a selector is
    /// installed rather than read as a request for the package manager or
    /// the runtime that shares its name.
    pub(crate) purl_selectors: Vec<String>,
    pub(crate) ecosystem_packages: Vec<EcosystemPackageSpecifier>,
}

impl PackageSpecifierPlan {
    pub(crate) fn parse(package_names: &[String]) -> Result<Self> {
        let mut node_packages = Vec::new();
        let mut purl_selectors = Vec::new();
        let mut ecosystem_packages = Vec::new();
        for package_name in package_names {
            match parse_specifier(package_name)? {
                ParsedSpecifier::Node { selector, from_purl } => {
                    if from_purl {
                        purl_selectors.push(selector.clone());
                    }
                    node_packages.push(selector);
                }
                ParsedSpecifier::Ecosystem(package) => ecosystem_packages.push(package),
            }
        }
        Ok(Self { node_packages, purl_selectors, ecosystem_packages })
    }

    pub(crate) fn has_cargo(&self) -> bool {
        self.ecosystem_packages
            .iter()
            .any(|specifier| matches!(specifier, EcosystemPackageSpecifier::Cargo(_)))
    }

    pub(crate) fn has_python(&self) -> bool {
        self.ecosystem_packages
            .iter()
            .any(|specifier| matches!(specifier, EcosystemPackageSpecifier::Python(_)))
    }
}

/// A selector as a diagnostic prints it.
///
/// The text comes from the command line, and a purl can carry credentials in
/// a `repository_url` qualifier and control characters anywhere. Parsing
/// reads the raw specifier; only this form reaches a message, and it
/// redacts on the way out so a rejected selector cannot leak a password or
/// drive the terminal.
#[derive(Clone, Copy)]
pub(super) struct Shown<'a>(pub(super) &'a str);

impl Shown<'_> {
    /// The selector up to its first qualifier or subpath separator.
    ///
    /// `redact_and_sanitize` recognizes an authority only when the selector
    /// spells `://user:pass@` literally, and a purl percent-encodes those
    /// separators, so a `repository_url` qualifier can carry a password past
    /// it. Both components are rejected whole, so naming the package the
    /// selector asked for says everything the message needs.
    pub(super) fn without_qualifiers(self) -> Self {
        match self.0.find(['?', '#']) {
            Some(separator) => Self(&self.0[..separator]),
            None => self,
        }
    }
}

impl std::fmt::Display for Shown<'_> {
    /// `redact_and_sanitize` reads an authority only where the text spells
    /// `://user:pass@` literally, and every component of a purl may be
    /// percent-encoded, so the decision is made on the decoded text: a
    /// selector that encodes any separator hides its credentials from a
    /// check made on the raw text alone. A selector with nothing to redact
    /// is quoted as it was written, which is what tells a reader that a
    /// component was encoded.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let decoded = without_authority_breaks(&percent_decode_str(self.0).decode_utf8_lossy());
        let redacted = pnpm_network::redact_and_sanitize(&decoded);
        if redacted == decoded {
            return formatter.write_str(&pnpm_network::redact_and_sanitize(self.0));
        }
        formatter.write_str(&redacted)
    }
}

/// `text` without the characters a URL authority cannot hold.
///
/// Decoding a selector can put one of them inside `://user:pass@`: `%20`
/// decodes to a space, and a byte that is not valid UTF-8 decodes to the
/// replacement character. Either splits the authority across the scan that
/// looks for it, which is why `redact_and_sanitize` drops control characters
/// before its own scan.
fn without_authority_breaks(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace() && *character != char::REPLACEMENT_CHARACTER)
        .collect()
}

/// One `pnpm add` selector once its protocol has been resolved.
enum ParsedSpecifier {
    Node { selector: String, from_purl: bool },
    Ecosystem(EcosystemPackageSpecifier),
}

fn parse_specifier(specifier: &str) -> Result<ParsedSpecifier> {
    let source = Shown(specifier);
    if let Some(rest) = specifier.strip_prefix(CARGO_PROTOCOL) {
        return parse_registry_specifier(rest, source).map(cargo_specifier);
    }
    if let Some(rest) = specifier.strip_prefix(PYTHON_PROTOCOL) {
        return parse_python_specifier(rest, source).map(python_specifier);
    }
    let Some(body) = purl::strip_scheme(specifier) else {
        return Ok(ParsedSpecifier::Node { selector: specifier.to_string(), from_purl: false });
    };
    parse_purl(&Purl::parse(body, source)?, source)
}

/// Routes a Package URL to the ecosystem its type names, spelling it the way
/// that ecosystem's own protocol would.
///
/// Every ecosystem here builds its selector by joining the purl's decoded
/// components, and each spelling has a grammar of its own: `name@spec` is an
/// npm alias, and a PEP 508 requirement carries extras and markers. Each
/// component is therefore validated before it is joined, so a decoded
/// separator cannot rewrite the selector into one for another package.
///
/// A purl version names one release rather than a range, so each arm rejects
/// a version its ecosystem would read as a range and pins the one it names.
fn parse_purl(purl: &Purl, source: Shown<'_>) -> Result<ParsedSpecifier> {
    match purl.package_type {
        PurlType::Npm => purl_node_specifier(purl, source)
            .map(|selector| ParsedSpecifier::Node { selector, from_purl: true }),
        PurlType::Cargo => purl_registry_specifier(purl, source).map(cargo_specifier),
        PurlType::Pypi => purl_python_specifier(purl, source).map(python_specifier),
    }
}

fn purl_node_specifier(purl: &Purl, source: Shown<'_>) -> Result<String> {
    let name = npm_name(purl, source)?;
    let Some(version) = purl.version.as_deref() else {
        return Ok(name);
    };
    if node_semver::Version::parse(version).is_err() {
        return Err(miette::miette!("{source} does not carry a valid npm version"));
    }
    Ok(format!("{name}@{version}"))
}

fn npm_name(purl: &Purl, source: Shown<'_>) -> Result<String> {
    let name = match &purl.namespace {
        Some(namespace) => format!("{}/{}", npm_scope(namespace, source)?, purl.name),
        None => purl.name.clone(),
    };
    if pnpm_package_name::is_valid_old_npm_package_name(&name) {
        return Ok(name);
    }
    Err(miette::miette!("{source} does not name a valid npm package"))
}

/// An npm scope carries a leading `@`. A Package URL percent-encodes it into
/// the namespace, so `pkg:npm/%40babel/core` is the canonical spelling of
/// `@babel/core`, but producers often drop it and both are accepted here.
fn npm_scope(namespace: &str, source: Shown<'_>) -> Result<String> {
    if namespace.contains('/') {
        return Err(miette::miette!(
            "{source} has a multi-segment purl namespace, but an npm scope is a single segment"
        ));
    }
    if namespace.starts_with('@') {
        return Ok(namespace.to_string());
    }
    Ok(format!("@{namespace}"))
}

/// Cargo reads a bare version as a caret range, so the pin needs a leading
/// `=` that the `crate:` spelling does not.
fn purl_registry_specifier(purl: &Purl, source: Shown<'_>) -> Result<RegistryPackageSpecifier> {
    reject_namespace(purl, source)?;
    reject_invalid_cargo_name(&purl.name, source)?;
    let Some(version) = &purl.version else {
        return parse_registry_specifier(&purl.name, source);
    };
    if semver::Version::parse(version).is_err() {
        return Err(miette::miette!("{source} does not carry a valid Cargo version"));
    }
    parse_registry_specifier(&format!("{}@={version}", purl.name), source)
}

fn purl_python_specifier(purl: &Purl, source: Shown<'_>) -> Result<String> {
    reject_namespace(purl, source)?;
    let name = pypi_project_name(purl, source)?;
    let Some(version) = &purl.version else {
        return python_requirement(name);
    };
    if version.parse::<pep440_rs::Version>().is_err() {
        return Err(miette::miette!("{source} does not carry a valid PyPI version"));
    }
    python_requirement(&format!("{name}=={version}"))
}

/// A project name as PEP 508 spells it: ASCII alphanumerics joined by
/// `.`, `-`, or `_`, starting and ending with an alphanumeric.
fn pypi_project_name<'a>(purl: &'a Purl, source: Shown<'_>) -> Result<&'a str> {
    let name = purl.name.as_str();
    if name.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        && name.ends_with(|ch: char| ch.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Ok(name);
    }
    Err(miette::miette!("{source} does not name a valid PyPI project"))
}

fn reject_namespace(purl: &Purl, source: Shown<'_>) -> Result<()> {
    if purl.namespace.is_some() {
        let package_type = &purl.package_type;
        return Err(miette::miette!(
            "{source} has a purl namespace, which type `{package_type}` does not define"
        ));
    }
    Ok(())
}

fn cargo_specifier(package: RegistryPackageSpecifier) -> ParsedSpecifier {
    ParsedSpecifier::Ecosystem(EcosystemPackageSpecifier::Cargo(package))
}

fn python_specifier(requirement: String) -> ParsedSpecifier {
    ParsedSpecifier::Ecosystem(EcosystemPackageSpecifier::Python(requirement))
}

/// A `pypi:` specifier as a PEP 508 requirement. pnpm spells a pinned
/// version `name@version`, which becomes an exact pin unless the version
/// already carries its own comparison operator.
fn parse_python_specifier(specifier: &str, source: Shown<'_>) -> Result<String> {
    let Some((name, version)) = specifier.rsplit_once('@') else {
        return python_requirement(specifier);
    };
    if version.is_empty() {
        return Err(miette::miette!("missing version after `@` in {source}"));
    }
    let operator = if version.starts_with(['<', '>', '=', '!', '~']) { "" } else { "==" };
    python_requirement(&format!("{name}{operator}{version}"))
}

fn python_requirement(requirement: &str) -> Result<String> {
    Ok(pnpm_python_resolver::parse_requirement(requirement)?.to_string())
}

/// A crates.io package name: ASCII alphanumerics joined by `-` or `_`.
///
/// A purl carries the name as its own component, so it is checked before
/// the `@` is appended: a decoded `@` would otherwise read back as the
/// version separator and name a different crate.
fn reject_invalid_cargo_name(name: &str, source: Shown<'_>) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(miette::miette!("invalid Cargo package name in {source}"));
    }
    Ok(())
}

fn parse_registry_specifier(
    specifier: &str,
    source: Shown<'_>,
) -> Result<RegistryPackageSpecifier> {
    let (name, version_spec) = specifier
        .rsplit_once('@')
        .map_or((specifier, None), |(name, version)| (name, Some(version)));
    reject_invalid_cargo_name(name, source)?;
    if version_spec == Some("") {
        return Err(miette::miette!("missing version after `@` in {source}"));
    }
    if let Some(version) = version_spec {
        if version.contains(':') {
            return Err(miette::miette!(
                "{source} is not supported by the crates.io-only proof of concept"
            ));
        }
        semver::VersionReq::parse(version)
            .map_err(|_| miette::miette!("invalid Cargo version requirement in {source}"))?;
    }
    Ok(RegistryPackageSpecifier {
        name: name.to_string(),
        version_spec: version_spec.map(str::to_string),
    })
}

#[cfg(test)]
mod tests;
