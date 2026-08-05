use super::deprecate::{
    DeprecateContext, DeprecateError, PackageSpec, auth_header_for_registry, fetch_package_meta,
    package_url, parse_package_spec, registry_for_package, registry_operation_error,
    write_error_from_response,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_config::Config;
use pacquet_network::send_with_retry;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Remove a package, or a range of its versions, from the registry.
///
/// A bare package name removes every published version and requires
/// --force; a pkg@range spec removes only the matching versions,
/// re-pointing dist-tags that referenced them.
#[derive(Debug, Args)]
pub struct UnpublishArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor
    /// authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Removes the package from the registry regardless of what version is
    /// currently published. Without this flag, pnpm refuses to unpublish an
    /// entire package.
    #[clap(long)]
    pub force: bool,

    /// The package to remove, optionally with a version range (pkg@1.x).
    pub params: Vec<String>,
}

/// Errors specific to `pnpm unpublish`. Codes and messages match the
/// TypeScript CLI; the registry-communication errors are shared with
/// [`DeprecateError`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UnpublishError {
    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_REQUIRED))]
    PackageRequired,

    #[display(
        "Run pnpm unpublish --force to remove all published versions of {package_name} ({versions_list}) from the registry.\nThis is a protection mechanism to prevent accidental unpublish of packages with many versions.\nIf you want to unpublish a specific version, run pnpm unpublish {package_name}@<version>"
    )]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_CONFIRM))]
    ConfirmRequired {
        #[error(not(source))]
        package_name: String,
        #[error(not(source))]
        versions_list: String,
    },

    #[display(
        "This package cannot be completely unpublished. Deprecate it instead or contact npm support."
    )]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_FORBIDDEN))]
    CompletelyForbidden,
}

/// The full packument as the registry returns it: the fields unpublish
/// mutates are typed, everything else round-trips through `other` so the
/// `PUT` sends the document back unchanged.
#[derive(Debug, Serialize, Deserialize)]
struct Packument {
    name: String,
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    // `serde_json::Map` preserves the registry's key order (the workspace
    // enables `preserve_order`), so the confirm-message version list and the
    // `PUT` body keep the packument's own ordering like the TypeScript CLI.
    #[serde(rename = "dist-tags", default)]
    dist_tags: Map<String, Value>,
    #[serde(default)]
    versions: Map<String, Value>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

impl UnpublishArgs {
    pub async fn run(self, config: &Config) -> miette::Result<Option<String>> {
        let context = DeprecateContext::new(config, self.registry.as_ref(), self.otp.clone())?;

        let spec = self.params.first().ok_or(UnpublishError::PackageRequired)?;
        let PackageSpec { name: package_name, version: version_range } = parse_package_spec(spec)?;

        let registry_url = registry_for_package(&context, &package_name);
        let auth_header = auth_header_for_registry(&context, &registry_url, &package_name);
        let package_url = package_url(&package_name, &registry_url)?;

        let pkg: Packument =
            fetch_package_meta(&context, &package_url, auth_header.as_deref(), &package_name)
                .await?;
        if pkg.versions.is_empty() {
            return Err(DeprecateError::NoVersions { package_name }.into());
        }

        let Some(range) = version_range else {
            return self
                .unpublish_all(&context, &package_url, &pkg, auth_header.as_deref())
                .await
                .map(Some);
        };

        let versions_to_unpublish = versions_matching_range(&pkg.versions, &range);
        if versions_to_unpublish.is_empty() {
            return Err(DeprecateError::NoMatchingVersions { version_range: range }.into());
        }

        // Removing every version is a full unpublish, protections included.
        if versions_to_unpublish.len() == pkg.versions.len() {
            return self
                .unpublish_all(&context, &package_url, &pkg, auth_header.as_deref())
                .await
                .map(Some);
        }

        unpublish_versions(
            &context,
            &package_url,
            &registry_url,
            pkg,
            &versions_to_unpublish,
            auth_header.as_deref(),
        )
        .await
        .map(Some)
    }

    /// Delete the whole packument (`DELETE <package>/-rev/<rev>`). Refused
    /// without `--force`; a 405 from the registry means the package may only
    /// be deprecated, not removed.
    async fn unpublish_all(
        &self,
        context: &DeprecateContext<'_>,
        package_url: &str,
        pkg: &Packument,
        auth_header: Option<&str>,
    ) -> miette::Result<String> {
        if !self.force {
            return Err(UnpublishError::ConfirmRequired {
                package_name: pkg.name.clone(),
                versions_list: pkg.versions.keys().cloned().collect::<Vec<_>>().join(", "),
            }
            .into());
        }

        let url = format!("{package_url}/-rev/{}", rev_str(pkg.rev.as_deref()));
        let response = send_delete(context, &url, auth_header).await?;
        if !response.status().is_success() {
            if response.status() == StatusCode::METHOD_NOT_ALLOWED {
                return Err(UnpublishError::CompletelyForbidden.into());
            }
            write_error_from_response(response, "unpublish".to_string()).await?;
        }

        Ok(format!(
            "Successfully unpublished all {} version(s) of {}",
            pkg.versions.len(),
            pkg.name,
        ))
    }
}

/// Remove `versions` from the packument, re-point the dist-tags that
/// referenced them, `PUT` the updated document back, then delete the
/// orphaned tarballs (a 404 there is fine — some registries clean tarballs
/// up on the packument update themselves).
async fn unpublish_versions(
    context: &DeprecateContext<'_>,
    package_url: &str,
    registry_url: &str,
    mut pkg: Packument,
    versions: &[String],
    auth_header: Option<&str>,
) -> miette::Result<String> {
    let mut tarballs: Vec<String> = Vec::new();
    for version in versions {
        let tarball = pkg
            .versions
            .get(version)
            .and_then(|data| data.get("dist"))
            .and_then(|dist| dist.get("tarball"))
            .and_then(Value::as_str);
        if let Some(tarball) = tarball {
            tarballs.push(tarball.to_string());
        }
        pkg.versions.remove(version);
    }

    let removed: HashSet<&str> = versions.iter().map(String::as_str).collect();
    let latest_was_removed = pkg
        .dist_tags
        .get("latest")
        .and_then(Value::as_str)
        .is_some_and(|latest| removed.contains(latest));
    pkg.dist_tags
        .retain(|_, target| !target.as_str().is_some_and(|target| removed.contains(target)));
    if latest_was_removed && let Some(highest) = highest_version(&pkg.versions) {
        pkg.dist_tags.insert("latest".to_string(), Value::String(highest));
    }

    // Internal CouchDB metadata must not round-trip into the PUT.
    pkg.other.remove("_revisions");
    pkg.other.remove("_attachments");

    let put_url = format!("{package_url}/-rev/{}", rev_str(pkg.rev.as_deref()));
    put_packument(context, &put_url, &pkg, auth_header).await?;

    let registry_origin = registry_origin(registry_url)?;
    for tarball in &tarballs {
        // Every delete bumps the packument revision; refetch for the current
        // one like the TypeScript CLI does.
        let updated: Packument =
            fetch_package_meta(context, package_url, auth_header, &pkg.name).await?;
        let pathname = tarball_pathname(tarball, registry_url)?;
        let url = format!("{registry_origin}/{pathname}/-rev/{}", rev_str(updated.rev.as_deref()));
        let response = send_delete(context, &url, auth_header).await?;
        if !response.status().is_success() && response.status() != StatusCode::NOT_FOUND {
            write_error_from_response(response, "unpublish".to_string()).await?;
        }
    }

    Ok(format!("Successfully unpublished {} version(s) of {}", versions.len(), pkg.name))
}

async fn put_packument(
    context: &DeprecateContext<'_>,
    url: &str,
    pkg: &Packument,
    auth_header: Option<&str>,
) -> miette::Result<()> {
    let body = serde_json::to_string(pkg).expect("a struct serializes");
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            let mut builder =
                client.put(url).header("content-type", "application/json").body(body.clone());
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            if let Some(otp) = context.otp.as_deref() {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .map_err(|source| {
            registry_operation_error("requesting the registry put endpoint", source)
        })?;
    if response.status().is_success() {
        return Ok(());
    }
    write_error_from_response(response, "unpublish".to_string()).await
}

async fn send_delete(
    context: &DeprecateContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> miette::Result<reqwest::Response> {
    let (_guard, response) =
        send_with_retry(&context.http_client, url, context.retry_opts, |client| {
            let mut builder = client.delete(url);
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            if let Some(otp) = context.otp.as_deref() {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .map_err(|source| {
            registry_operation_error("requesting the registry delete endpoint", source)
        })?;
    Ok(response)
}

/// The `-rev` path segment. A packument without `_rev` renders as the
/// literal `undefined`, exactly like the TypeScript template string does.
fn rev_str(rev: Option<&str>) -> &str {
    rev.unwrap_or("undefined")
}

/// The version keys `range` matches. An unparsable range matches nothing,
/// mirroring `semver.satisfies`.
fn versions_matching_range(versions: &Map<String, Value>, range: &str) -> Vec<String> {
    match Range::parse(range) {
        Ok(range) => versions
            .keys()
            .filter(|ver_str| Version::parse(ver_str).is_ok_and(|ver| range.satisfies(&ver)))
            .cloned()
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The highest remaining semver version — the new `latest` after the old
/// one is unpublished.
fn highest_version(versions: &Map<String, Value>) -> Option<String> {
    versions
        .keys()
        .filter_map(|ver_str| Version::parse(ver_str).ok().map(|ver| (ver, ver_str)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, ver_str)| ver_str.clone())
}

/// The `scheme://host[:port]` origin of the registry, which tarball URLs
/// are deleted relative to.
fn registry_origin(registry_url: &str) -> miette::Result<String> {
    reqwest::Url::parse(registry_url)
        .map(|url| url.origin().ascii_serialization())
        .map_err(|source| registry_operation_error("build registry URL", source))
}

/// The tarball's pathname with the registry's own path prefix stripped, so
/// registries mounted under a path delete the right resource.
fn tarball_pathname(tarball_url: &str, registry_url: &str) -> miette::Result<String> {
    let registry_path = reqwest::Url::parse(registry_url)
        .map_err(|source| registry_operation_error("build registry URL", source))?
        .path()
        .trim_start_matches('/')
        .to_string();
    let tarball_path = reqwest::Url::parse(tarball_url)
        .map_err(|source| registry_operation_error("build tarball URL", source))?
        .path()
        .trim_start_matches('/')
        .to_string();
    Ok(match tarball_path.strip_prefix(&registry_path) {
        Some(stripped) if !registry_path.is_empty() => stripped.to_string(),
        _ => tarball_path,
    })
}

#[cfg(test)]
mod tests;
