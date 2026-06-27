//! Port of the option-assembly half of `publishPackedPkg.ts`: choose the
//! target registry, resolve the access level, and run the per-package OIDC
//! token / provenance exchange that takes precedence over static credentials.

use std::collections::BTreeMap;

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::Reporter;
use serde_json::Value;

use crate::{
    capabilities::{CiInfo, Clock, EnvVar, OidcFetch},
    display_error::display_diagnostic,
    global_log::global_warn,
    oidc::{
        DetermineProvenanceError, GetIdTokenError, OidcHttpOptions, determine_provenance,
        fetch_auth_token, get_id_token,
    },
    registry_config_keys::{NormalizedRegistryUrl, parse_supported_registry_url},
};

/// The package access level the registry should record. Ports the
/// `'public' | 'restricted'` union; `None` leaves it unset (the registry
/// default). Models the TS string-literal union as a closed enum.
#[derive(Debug, derive_more::Display, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    #[display("public")]
    Public,
    #[display("restricted")]
    Restricted,
}

impl Access {
    /// Parse the CLI / `publishConfig.access` value. Ports `isPublishAccess`:
    /// only `public` / `restricted` are accepted, anything else is `None`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Access> {
        match value {
            "public" => Some(Access::Public),
            "restricted" => Some(Access::Restricted),
            _ => None,
        }
    }
}

/// The registry has a protocol `pnpm publish` cannot use. Ports pnpm's
/// `PublishUnsupportedRegistryProtocolError`
/// (`ERR_PNPM_PUBLISH_UNSUPPORTED_REGISTRY_PROTOCOL`).
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("Registry {registry_url} has an unsupported protocol")]
#[diagnostic(
    code(ERR_PNPM_PUBLISH_UNSUPPORTED_REGISTRY_PROTOCOL),
    help("`pnpm publish` only supports HTTP and HTTPS registries")
)]
pub struct PublishUnsupportedRegistryProtocolError {
    pub registry_url: String,
}

/// Find the target registry for a package. The manifest's
/// `publishConfig.registry` wins, then a scoped registry for the package's
/// scope, then the default registry. Ports the registry-selection half of TS
/// `findRegistryInfo` (credential / TLS resolution is handled by pacquet's
/// shared [`pacquet_network::AuthHeaders`] at request time).
pub fn find_registry_info(
    name: &str,
    default_registry: &str,
    scoped_registries: &BTreeMap<String, String>,
    publish_config_registry: Option<&str>,
) -> Result<NormalizedRegistryUrl, PublishUnsupportedRegistryProtocolError> {
    let scoped_registry =
        scope_of(name).and_then(|scope| scoped_registries.get(&format!("@{scope}")));
    let non_normalized = publish_config_registry
        .map(str::to_owned)
        .or_else(|| scoped_registry.cloned())
        .unwrap_or_else(|| default_registry.to_owned());

    parse_supported_registry_url(&non_normalized)
        .map(|info| info.normalized_url)
        .ok_or(PublishUnsupportedRegistryProtocolError { registry_url: non_normalized })
}

/// The scope of a package name (`@scope/name` → `scope`), or `None` when
/// unscoped. Mirrors the TS `@(?<scope>[^/]+)/(?<slug>[^/]+)` match.
fn scope_of(name: &str) -> Option<&str> {
    let rest = name.strip_prefix('@')?;
    let slash = rest.find('/')?;
    let scope = &rest[..slash];
    let slug = &rest[slash + 1..];
    (!scope.is_empty() && !slug.is_empty()).then_some(scope)
}

/// Resolve the access level: an explicit `--access` wins, else a valid
/// `publishConfig.access`, else `None`. Mirrors the access line of
/// `createPublishOptions`.
#[must_use]
pub fn resolve_access(explicit: Option<Access>, manifest: &Value) -> Option<Access> {
    explicit.or_else(|| {
        manifest
            .get("publishConfig")
            .and_then(|config| config.get("access"))
            .and_then(Value::as_str)
            .and_then(Access::parse)
    })
}

/// The OIDC-derived auth token and provenance flag, returned when trusted
/// publishing is configured for the package. Ports TS
/// `OidcTokenProvenanceResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcTokenProvenance {
    pub auth_token: String,
    pub provenance: Option<bool>,
}

/// A non-skippable OIDC failure: the id-token request or provenance step hit a
/// hard transport / parse error that the TS code lets propagate past its
/// `instanceof` guards.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum FetchTokenAndProvenanceError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    IdToken(GetIdTokenError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Provenance(DetermineProvenanceError),
}

/// Try to obtain an auth token (and provenance flag) for `package_name` on
/// `registry` via an OIDC token exchange. Returns `Ok(None)` when OIDC is not
/// applicable or fails in a skippable way — the caller then falls back to
/// static credentials. Ports TS `fetchTokenAndProvenanceByOidc`.
///
/// `provenance_override` is the explicit `--provenance` value: when set, it is
/// used verbatim and the visibility probe is skipped.
pub async fn fetch_token_and_provenance_by_oidc<Sys, Reporter>(
    package_name: &str,
    registry: &str,
    provenance_override: Option<bool>,
    http: &OidcHttpOptions,
) -> Result<Option<OidcTokenProvenance>, FetchTokenAndProvenanceError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    let id_token = match get_id_token::<Sys, Reporter>(registry, http).await {
        Ok(token) => token,
        Err(GetIdTokenError::IdToken(error)) => {
            global_warn::<Reporter>(&format!("Skipped OIDC: {}", display_diagnostic(&error)));
            return Ok(None);
        }
        Err(error) => return Err(FetchTokenAndProvenanceError::IdToken(error)),
    };
    let Some(id_token) = id_token else {
        // OIDC is simply not applicable (local publish / non-OIDC CI). Stay
        // silent — only configuration errors in a supported CI warn.
        return Ok(None);
    };

    let auth_token = match fetch_auth_token::<Sys>(&id_token, package_name, registry, http).await {
        Ok(token) => token,
        Err(error) => {
            global_warn::<Reporter>(&format!("Skipped OIDC: {}", display_diagnostic(&error)));
            return Ok(None);
        }
    };

    if provenance_override.is_some() {
        return Ok(Some(OidcTokenProvenance { auth_token, provenance: provenance_override }));
    }

    match determine_provenance::<Sys>(&auth_token, &id_token, package_name, registry, http).await {
        Ok(provenance) => Ok(Some(OidcTokenProvenance { auth_token, provenance })),
        Err(DetermineProvenanceError::Provenance(error)) => {
            // Keep the OIDC auth token even when provenance can't be decided —
            // the publish itself can still go through, matching the npm CLI.
            global_warn::<Reporter>(&format!(
                "Skipped setting provenance: {}",
                display_diagnostic(&error),
            ));
            Ok(Some(OidcTokenProvenance { auth_token, provenance: None }))
        }
        Err(error) => Err(FetchTokenAndProvenanceError::Provenance(error)),
    }
}

/// The publish parameters [`create_publish_options`] resolves from. Ports the
/// subset of `PublishPackedPkgOptions` the option-assembly reads.
pub struct CreatePublishOptionsInput<'a> {
    pub default_registry: &'a str,
    pub scoped_registries: &'a BTreeMap<String, String>,
    pub access: Option<Access>,
    pub tag: &'a str,
    pub otp: Option<&'a str>,
    pub provenance: Option<bool>,
    pub http: &'a OidcHttpOptions,
}

/// The resolved registry, access, tag, OTP and provenance/auth-token values a
/// publish request needs. Ports the relevant fields of TS
/// `StagePublishOptions`.
#[derive(Debug, Clone)]
pub struct ResolvedPublishOptions {
    pub registry: NormalizedRegistryUrl,
    pub access: Option<Access>,
    pub default_tag: String,
    pub otp: Option<String>,
    pub provenance: Option<bool>,
    /// Set from a successful OIDC exchange; overrides the static auth header.
    pub auth_token_override: Option<String>,
}

/// Build the registry / auth / access options for publishing `manifest`. When
/// `oidc_enabled` is `false` the per-package OIDC exchange is skipped (batch
/// publish sends many packages a package-scoped token cannot authorize). Ports
/// the option-assembly portion of TS `createPublishOptions`.
pub async fn create_publish_options<Sys, Reporter>(
    manifest: &Value,
    input: &CreatePublishOptionsInput<'_>,
    oidc_enabled: bool,
) -> Result<ResolvedPublishOptions, CreatePublishOptionsError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    let publish_config_registry = manifest
        .get("publishConfig")
        .and_then(|config| config.get("registry"))
        .and_then(Value::as_str);
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
    let registry = find_registry_info(
        name,
        input.default_registry,
        input.scoped_registries,
        publish_config_registry,
    )?;

    let access = resolve_access(input.access, manifest);
    let mut provenance = input.provenance;
    let mut auth_token_override = None;

    if oidc_enabled {
        // OIDC takes precedence over a configured static token, mirroring the
        // npm CLI: trusted publishing wins when the registry has it configured.
        let oidc = fetch_token_and_provenance_by_oidc::<Sys, Reporter>(
            name,
            registry.as_str(),
            input.provenance,
            input.http,
        )
        .await?;
        if let Some(oidc) = oidc {
            auth_token_override = Some(oidc.auth_token);
            provenance = provenance.or(oidc.provenance);
        }
    }

    Ok(ResolvedPublishOptions {
        registry,
        access,
        default_tag: input.tag.to_owned(),
        otp: input.otp.map(str::to_owned),
        provenance,
        auth_token_override,
    })
}

/// Failure surface of [`create_publish_options`].
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum CreatePublishOptionsError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    UnsupportedProtocol(PublishUnsupportedRegistryProtocolError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Oidc(FetchTokenAndProvenanceError),
}

impl From<PublishUnsupportedRegistryProtocolError> for CreatePublishOptionsError {
    fn from(error: PublishUnsupportedRegistryProtocolError) -> Self {
        CreatePublishOptionsError::UnsupportedProtocol(error)
    }
}

impl From<FetchTokenAndProvenanceError> for CreatePublishOptionsError {
    fn from(error: FetchTokenAndProvenanceError) -> Self {
        CreatePublishOptionsError::Oidc(error)
    }
}

#[cfg(test)]
mod tests;
