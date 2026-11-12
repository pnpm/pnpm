//! The credential rules of a document the metadata database stores: upstream
//! credentials are `${VAR}` references to listed variables, resolved by each
//! replica when it builds the routing.

use pnpm_env_replace::{EnvVar, env_replace_lossy, placeholder_ranges};
use pnpr_error::RegistryError;

use super::{RegistryFile, RegistryGraphDocument};
use crate::config_file::UpstreamFile;

impl RegistryGraphDocument {
    /// Check the rules a stored document is held to: an upstream `auth.token`
    /// and every `headers` value are exactly one `${VAR}` reference, so the
    /// database holds no credential, and every variable an upstream's
    /// `auth.token`, `auth.tokenEnv`, or `headers` reference is listed in
    /// `registry_env` (`registryEnv` in the config file).
    pub fn check_stored(&self, registry_env: &[String]) -> Result<(), RegistryError> {
        for (name, entry) in &self.registries {
            if let RegistryFile::Upstream(upstream) = entry {
                check_stored_upstream(name, upstream, registry_env)?;
            }
        }
        Ok(())
    }

    /// This document with each upstream's `auth.token` and `headers` values
    /// substituted from the environment. A referenced variable that is unset
    /// is an error, so a replica never sends a credential it could not read.
    pub(super) fn with_credentials_resolved<Sys: EnvVar>(mut self) -> Result<Self, RegistryError> {
        for (name, entry) in &mut self.registries {
            if let RegistryFile::Upstream(upstream) = entry {
                resolve_upstream_credentials::<Sys>(name, upstream)?;
            }
        }
        Ok(self)
    }
}

/// Whether `name` can be an environment variable name in a reference.
pub(crate) fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|rest| rest == '_' || rest.is_ascii_alphanumeric())
}

fn check_stored_upstream(
    name: &str,
    upstream: &UpstreamFile,
    registry_env: &[String],
) -> Result<(), RegistryError> {
    if let Some(auth) = &upstream.auth {
        if let Some(token) = &auth.token {
            let variable = whole_reference(token)
                .ok_or_else(|| {
                    invalid_stored(name, "`auth.token` must be one `${VAR}` reference")
                })?;
            require_listed(name, "auth.token", variable, registry_env)?;
        }
        if let Some(variable) = auth.token_env.as_ref().and_then(|token_env| token_env.var_name()) {
            require_listed(name, "auth.tokenEnv", variable, registry_env)?;
        }
    }
    for (header, value) in &upstream.headers {
        let field = format!("headers.{header}");
        let variable = whole_reference(value)
            .ok_or_else(|| {
                invalid_stored(name, &format!("`{field}` must be one `${{VAR}}` reference"))
            })?;
        require_listed(name, &field, variable, registry_env)?;
    }
    Ok(())
}

fn resolve_upstream_credentials<Sys: EnvVar>(
    name: &str,
    upstream: &mut UpstreamFile,
) -> Result<(), RegistryError> {
    if let Some(token) = upstream.auth.as_mut().and_then(|auth| auth.token.as_mut()) {
        *token = substitute::<Sys>(name, token)?.into();
    }
    for value in upstream.headers.values_mut() {
        *value = substitute::<Sys>(name, value)?.into();
    }
    Ok(())
}

fn substitute<Sys: EnvVar>(name: &str, value: &str) -> Result<String, RegistryError> {
    let (substituted, unresolved) = env_replace_lossy::<Sys>(value);
    if unresolved.is_empty() {
        return Ok(substituted);
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "upstream registry {name:?} references unset environment variables: {unresolved:?}",
        ),
    })
}

/// The variable of `value` when the whole value is one `${VAR}` reference.
fn whole_reference(value: &str) -> Option<&str> {
    match placeholder_ranges(value).as_slice() {
        [range] if *range == (0..value.len()) => reference_variable(value),
        _ => None,
    }
}

/// The variable a `${VAR}` placeholder names. The default and optional forms
/// (`${VAR-default}`, `${VAR:-default}`, `${VAR?}`) are not accepted in a
/// stored document.
fn reference_variable(placeholder: &str) -> Option<&str> {
    placeholder
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|variable| is_env_name(variable))
}

fn require_listed(
    name: &str,
    field: &str,
    variable: &str,
    registry_env: &[String],
) -> Result<(), RegistryError> {
    if registry_env
        .iter()
        .any(|listed| listed == variable)
    {
        return Ok(());
    }
    Err(invalid_stored(
        name,
        &format!("`{field}` references {variable:?}, which `registryEnv` does not list"),
    ))
}

fn invalid_stored(name: &str, reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: format!("upstream registry {name:?}: {reason}") }
}
