use super::{Inputs, Registry, environment::PythonPrepare, manifest};
use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{Diagnostic, IntoDiagnostic, Result};

pub(crate) struct Index {
    pub(crate) url: url::Url,
    pub(crate) routes: Vec<(url::Url, pnpm_config::PythonRegistryRoute)>,
    pub(crate) auth: pnpm_network::AuthHeaders,
}

/// Python namespace claims with the machine's existing registry credentials.
pub(super) fn python_index(config: &pnpm_config::Config) -> Result<Index> {
    let configured = config.python_registry_indexes();
    let routes = pnpm_config::PythonRegistryRoute::from_indexes(&configured)
        .into_diagnostic()?
        .into_iter()
        .map(|route| Ok((parse_index(&route.url)?, route)))
        .collect::<Result<Vec<_>>>()?;
    let url = routes
        .iter()
        .find(|(_, route)| route.is_default())
        .unwrap_or(&routes[0])
        .0
        .clone();
    let auth = (*config.auth_headers).clone().with_secure_transport();
    Ok(Index { url, routes, auth })
}

impl Index {
    pub(super) fn select(&self, name: &str) -> Result<&url::Url> {
        self.routes
            .iter()
            .find(|(_, route)| !route.is_default() && route.matches(name))
            .or_else(|| self.routes.iter().find(|(_, route)| route.is_default()))
            .map(|(url, _)| url)
            .ok_or_else(|| UnclaimedPackage { name: name.to_string() }.into())
    }

    fn package_routes(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        self.routes
            .iter()
            .map(|(url, route)| (url.to_string(), route.packages()))
            .collect()
    }

    pub(super) fn can_resolve_remotely(&self) -> bool {
        self.routes.len() == 1 && self.routes[0].1.is_default()
    }

    pub(super) fn cache_key(&self, url: &url::Url) -> String {
        let key = self.auth
            .for_secure_url(url.as_str())
            .map_or_else(|| url.to_string(), |header| format!("{url}\0{header}"));
        pnpm_crypto_hash::create_hex_hash(&key)
    }
}

fn parse_index(configured: &str) -> Result<url::Url> {
    let mut index: url::Url = configured.parse().into_diagnostic()?;
    pnpm_python_resolver::validate_url(&index)?;
    // Every Simple API request is `index.join("<distribution>/")`, which
    // replaces the last path segment when the base has no trailing slash.
    // The `registries` key is normalized as a string, so a URL carrying a
    // query reaches here with the slash on the query instead.
    if !index.path().ends_with('/') {
        index.set_path(&format!("{}/", index.path()));
    }
    Ok(index)
}

impl PythonPrepare<'_> {
    pub(super) fn configured_registry(
        &self,
        requirements: &[pep508_rs::Requirement],
        rules: &manifest::Manifest,
    ) -> Result<(Registry<'_>, Inputs)> {
        let mut registry = self.registry();
        let mut inputs = self.inputs(requirements);
        self.configure_resolution(&mut registry.resolution.packages, &mut inputs, rules)?;
        Ok((registry, inputs))
    }

    fn configure_resolution(
        &self,
        packages: &mut pnpm_python_resolver::Packages,
        inputs: &mut Inputs,
        rules: &manifest::Manifest,
    ) -> Result<()> {
        let config = self.context.config;
        packages.overrides = config.python.overrides
            .iter()
            .chain(&rules.tool.uv.overrides)
            .map(|requirement| parse_rule(requirement))
            .collect::<Result<_>>()?;
        packages.constraints = config.python.constraints
            .iter()
            .chain(&rules.tool.uv.constraints)
            .map(|requirement| parse_rule(requirement))
            .collect::<Result<_>>()?;
        inputs.set_resolution_settings(&[], &packages.overrides, &packages.constraints);
        if !self.index.can_resolve_remotely() {
            inputs.set_registry_packages(self.index.package_routes());
        }
        Ok(())
    }

    /// What this install's resolution depends on, which is what decides
    /// whether the lockfile on disk still answers it.
    fn inputs(&self, requirements: &[pep508_rs::Requirement]) -> Inputs {
        if self.environments.declared {
            Inputs::declared(
                requirements,
                &self.environments.platforms,
                &self.environments.python_versions,
                self.index.url.as_str(),
            )
        } else {
            Inputs::new(requirements, &self.interpreter.target, self.index.url.as_str())
        }
    }
}

fn parse_rule(declared: &str) -> Result<pep508_rs::Requirement> {
    let requirement = pnpm_python_resolver::parse_requirement(declared)?;
    if matches!(requirement.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_))) {
        return Err(UnsupportedRule { requirement: declared.to_string() }.into());
    }
    Ok(requirement)
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Python overrides and constraints must use registry version requirements: {requirement}")]
#[diagnostic(code(ERR_PNPM_UNSUPPORTED_PYTHON_RULE))]
struct UnsupportedRule {
    requirement: String,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("No Python registry claims package {name:?}")]
#[diagnostic(code(ERR_PNPM_UNCLAIMED_PYTHON_PACKAGE))]
struct UnclaimedPackage {
    name: String,
}

#[cfg(test)]
mod tests;
