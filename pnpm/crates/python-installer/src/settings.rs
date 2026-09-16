use super::{Inputs, Registry, environment::PythonPrepare, manifest};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use miette::{IntoDiagnostic, Result};

/// The Python index a project resolves against. Credentials the
/// configured URL carried are lifted into `auth`, so `url` never holds
/// any: it is cached under, and locked as, what it reads.
pub(crate) struct Index {
    pub(crate) url: url::Url,
    pub(crate) extra_urls: Vec<url::Url>,
    pub(crate) auth: pnpm_network::AuthHeaders,
}

/// The configured Python index, with any credentials it carries lifted out
/// of the URL. A repository-selected Python index must not select
/// user-level npm credentials.
pub(super) fn python_index(config: &pnpm_config::Config) -> Result<Index> {
    let mut auth = pnpm_network::AuthHeaders::default().with_secure_transport();
    let mut urls = Vec::new();
    for configured in
        std::iter::once(&config.python.index_url).chain(&config.python.extra_index_urls)
    {
        let mut index: url::Url = configured.parse().into_diagnostic()?;
        if !index.username().is_empty() || index.password().is_some() {
            let username = pnpm_network::percent_decode_str(index.username());
            let password = pnpm_network::percent_decode_str(index.password().unwrap_or(""));
            index
                .set_username("")
                .map_err(|()| miette::miette!("invalid Python index URL"))?;
            index
                .set_password(None)
                .map_err(|()| miette::miette!("invalid Python index URL"))?;
            auth.insert_url_header(
                index.as_str(),
                format!("Basic {}", STANDARD.encode(format!("{username}:{password}"))),
            );
        }
        pnpm_python_resolver::validate_url(&index)?;
        if !index.path().ends_with('/') {
            index.set_path(&format!("{}/", index.path()));
        }
        urls.push(index);
    }
    let url = urls.remove(0);
    Ok(Index { url, extra_urls: urls, auth })
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
        miette::bail!(
            "Python overrides and constraints must use registry version requirements: {declared}",
        );
    }
    Ok(requirement)
}
