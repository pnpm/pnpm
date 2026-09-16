use super::{Inputs, Registry, environment::PythonPrepare, manifest};
use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{Diagnostic, IntoDiagnostic, Result};
use std::{collections::HashMap, sync::Arc};

pub(crate) struct Index {
    pub(crate) url: url::Url,
    pub(crate) extra_urls: Vec<url::Url>,
    pub(crate) auth: pnpm_network::AuthHeaders,
}

/// Repository-selected Python indexes must not select user-level npm credentials.
pub(super) fn python_index(config: &pnpm_config::Config) -> Result<Index> {
    let indexes = std::iter::once(&config.python.index_url)
        .chain(&config.python.extra_index_urls)
        .map(|configured| parse_index(configured))
        .collect::<Result<Vec<_>>>()?;
    validate_index_credentials(&indexes)?;
    let url = indexes[0].url.clone();
    let extra_urls = indexes
        .iter()
        .skip(1)
        .map(|index| index.url.clone())
        .collect();
    let auth = pnpm_network::AuthHeaders::default()
        .with_secure_transport()
        .with_route_hook(Arc::new(IndexAuth { indexes }));
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

struct ConfiguredIndex {
    url: url::Url,
    authorization: Option<String>,
}

struct IndexAuth {
    indexes: Vec<ConfiguredIndex>,
}

impl pnpm_network::UpstreamRouteHook for IndexAuth {
    fn authorize(&self, url: &str, _package: Option<&str>) -> Option<String> {
        let request = url::Url::parse(url).ok()?;
        self.indexes
            .iter()
            .filter(|index| {
                request.origin() == index.url.origin()
                    && request.path().starts_with(index.url.path())
            })
            .max_by_key(|index| index.url.path().len())
            .and_then(|index| index.authorization.clone())
    }
}

fn validate_index_credentials(indexes: &[ConfiguredIndex]) -> Result<()> {
    let mut credentials = HashMap::new();
    for index in indexes {
        let origin = index.url.origin();
        if credentials
            .insert((origin.clone(), index.url.path()), &index.authorization)
            .is_some_and(|previous| previous != &index.authorization)
        {
            return Err(ConflictingIndexCredentials {
                url: format!("{}{}", origin.ascii_serialization(), index.url.path()),
            }
            .into());
        }
    }
    Ok(())
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Python index {url} is configured with conflicting credentials")]
#[diagnostic(code(ERR_PNPM_CONFLICTING_PYTHON_INDEX_CREDENTIALS))]
struct ConflictingIndexCredentials {
    url: String,
}

fn parse_index(configured: &str) -> Result<ConfiguredIndex> {
    let mut index: url::Url = configured.parse().into_diagnostic()?;
    let authorization = if !index.username().is_empty() || index.password().is_some() {
        let username = pnpm_network::percent_decode_str(index.username());
        let password = pnpm_network::percent_decode_str(index.password().unwrap_or(""));
        index
            .set_username("")
            .map_err(|()| miette::miette!("invalid Python index URL"))?;
        index
            .set_password(None)
            .map_err(|()| miette::miette!("invalid Python index URL"))?;
        Some(format!("Basic {}", pnpm_network::base64_encode(&format!("{username}:{password}"))))
    } else {
        None
    };
    pnpm_python_resolver::validate_url(&index)?;
    if !index.path().ends_with('/') {
        index.set_path(&format!("{}/", index.path()));
    }
    Ok(ConfiguredIndex { url: index, authorization })
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
