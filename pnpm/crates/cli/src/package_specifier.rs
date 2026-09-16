mod purl;

use miette::Result;
use purl::Purl;

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
    pub(crate) ecosystem_packages: Vec<EcosystemPackageSpecifier>,
}

impl PackageSpecifierPlan {
    pub(crate) fn parse(package_names: &[String]) -> Result<Self> {
        let mut node_packages = Vec::new();
        let mut ecosystem_packages = Vec::new();
        for package_name in package_names {
            match parse_specifier(package_name)? {
                ParsedSpecifier::Node(package) => node_packages.push(package),
                ParsedSpecifier::Ecosystem(package) => ecosystem_packages.push(package),
            }
        }
        Ok(Self { node_packages, ecosystem_packages })
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

/// One `pnpm add` selector once its protocol has been resolved.
enum ParsedSpecifier {
    Node(String),
    Ecosystem(EcosystemPackageSpecifier),
}

fn parse_specifier(specifier: &str) -> Result<ParsedSpecifier> {
    if let Some(rest) = specifier.strip_prefix(CARGO_PROTOCOL) {
        return parse_registry_specifier(rest, specifier).map(cargo_specifier);
    }
    if let Some(rest) = specifier.strip_prefix(PYTHON_PROTOCOL) {
        return parse_python_specifier(rest, specifier).map(python_specifier);
    }
    let Some(body) = purl::strip_scheme(specifier) else {
        return Ok(ParsedSpecifier::Node(specifier.to_string()));
    };
    parse_purl(&Purl::parse(body, specifier)?, specifier)
}

/// Routes a Package URL to the ecosystem its type names, spelling it the way
/// that ecosystem's own protocol would.
///
/// Every ecosystem here builds its selector by joining the purl's decoded
/// components, and each spelling has a grammar of its own: `name@spec` is an
/// npm alias, and a PEP 508 requirement carries extras and markers. Each
/// component is therefore validated before it is joined, so a decoded
/// separator cannot rewrite the selector into one for another package.
fn parse_purl(purl: &Purl, source: &str) -> Result<ParsedSpecifier> {
    match purl.package_type.as_str() {
        "npm" => purl_node_specifier(purl, source).map(ParsedSpecifier::Node),
        "cargo" => purl_registry_specifier(purl, source).map(cargo_specifier),
        "pypi" => purl_python_specifier(purl, source).map(python_specifier),
        package_type => Err(miette::miette!(
            "{source} has purl type `{package_type}`, but pnpm can add only `npm`, `cargo`, and `pypi` packages"
        )),
    }
}

fn purl_node_specifier(purl: &Purl, source: &str) -> Result<String> {
    let name = npm_name(purl, source)?;
    let Some(version) = purl.version.as_deref() else {
        return Ok(name);
    };
    if node_semver::Version::parse(version).is_err() {
        return Err(miette::miette!("{source} does not carry a valid npm version"));
    }
    Ok(format!("{name}@{version}"))
}

fn npm_name(purl: &Purl, source: &str) -> Result<String> {
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
fn npm_scope(namespace: &str, source: &str) -> Result<String> {
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

fn purl_registry_specifier(purl: &Purl, source: &str) -> Result<RegistryPackageSpecifier> {
    reject_namespace(purl, source)?;
    let specifier = match &purl.version {
        Some(version) => format!("{}@{version}", purl.name),
        None => purl.name.clone(),
    };
    parse_registry_specifier(&specifier, source)
}

/// A Package URL version is an exact version rather than a range, so it
/// becomes a pinned requirement.
fn purl_python_specifier(purl: &Purl, source: &str) -> Result<String> {
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
fn pypi_project_name<'a>(purl: &'a Purl, source: &str) -> Result<&'a str> {
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

fn reject_namespace(purl: &Purl, source: &str) -> Result<()> {
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
fn parse_python_specifier(specifier: &str, source: &str) -> Result<String> {
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

fn parse_registry_specifier(specifier: &str, source: &str) -> Result<RegistryPackageSpecifier> {
    let (name, version_spec) = specifier
        .rsplit_once('@')
        .map_or((specifier, None), |(name, version)| (name, Some(version)));
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(miette::miette!("invalid Cargo package name in {source}"));
    }
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
