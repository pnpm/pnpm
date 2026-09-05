use miette::Result;

const CARGO_PROTOCOL: &str = "crate:";

/// A package request routed to a non-Node.js ecosystem.
///
/// New language protocols belong here. The npm add path only receives the
/// selectors left in [`PackageSpecifierPlan::node_packages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EcosystemPackageSpecifier {
    Cargo(RegistryPackageSpecifier),
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
            if let Some(specifier) = package_name.strip_prefix(CARGO_PROTOCOL) {
                ecosystem_packages.push(EcosystemPackageSpecifier::Cargo(
                    parse_registry_specifier(specifier, CARGO_PROTOCOL)?,
                ));
            } else {
                node_packages.push(package_name.clone());
            }
        }
        Ok(Self { node_packages, ecosystem_packages })
    }

    pub(crate) fn has_cargo(&self) -> bool {
        self.ecosystem_packages
            .iter()
            .any(|specifier| matches!(specifier, EcosystemPackageSpecifier::Cargo(_)))
    }
}

fn parse_registry_specifier(
    specifier: &str,
    protocol: &'static str,
) -> Result<RegistryPackageSpecifier> {
    let (name, version_spec) = specifier
        .rsplit_once('@')
        .map_or((specifier, None), |(name, version)| (name, Some(version)));
    if name.is_empty()
        || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(miette::miette!("invalid {protocol} package name in {protocol}{specifier}"));
    }
    if version_spec == Some("") {
        return Err(miette::miette!("missing version after `@` in {protocol}{specifier}"));
    }
    if let Some(version) = version_spec {
        if version.contains(':') {
            return Err(miette::miette!(
                "{protocol}{specifier} is not supported by the crates.io-only proof of concept"
            ));
        }
        semver::VersionReq::parse(version).map_err(|_| {
            miette::miette!("invalid Cargo version requirement in {protocol}{specifier}")
        })?;
    }
    Ok(RegistryPackageSpecifier {
        name: name.to_string(),
        version_spec: version_spec.map(str::to_string),
    })
}

#[cfg(test)]
mod tests;
