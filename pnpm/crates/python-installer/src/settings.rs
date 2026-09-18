use super::{Inputs, Registry, environment::PythonPrepare, manifest};
use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{Diagnostic, IntoDiagnostic, Result};

pub(crate) struct Index {
    pub(crate) url: url::Url,
    pub(crate) extra_urls: Vec<url::Url>,
    pub(crate) auth: pnpm_network::AuthHeaders,
}

/// The indexes `registries` declares for `PyPI`, with the credentials the
/// machine holds for them.
///
/// Credentials are resolved by origin from the same auth sources every other
/// package source uses, which is why a `registries` key may carry none of its
/// own: the map lives in the committed `pnpm-workspace.yaml`.
pub(super) fn python_index(config: &pnpm_config::Config) -> Result<Index> {
    let mut indexes = config
        .python_indexes()
        .into_iter()
        .map(parse_index)
        .collect::<Result<Vec<_>>>()?;
    let extra_urls = indexes.split_off(1);
    let url = indexes.pop().expect("python_indexes answers with at least the default index");
    let auth = (*config.auth_headers).clone().with_secure_transport();
    Ok(Index { url, extra_urls, auth })
}

impl Index {
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
        inputs.set_resolution_settings(
            &self.index.extra_urls
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            &packages.overrides,
            &packages.constraints,
        );
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

#[cfg(test)]
mod tests;
