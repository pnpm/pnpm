pub use wire::*;

use std::collections::HashMap;

use pipe_trait::Pipe;
use pnpm_network::{AuthHeaders, ThrottledClient, normalize_registry_url};
use serde::{Deserialize, Serialize};

use crate::{NetworkError, PackageTag, RegistryError, package_distribution::PackageDistribution};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror npm registry version metadata."
    )
)]
pub struct PackageVersion {
    pub name: String,
    pub version: node_semver::Version,
    pub dist: PackageDistribution,
    #[serde(
        default,
        deserialize_with = "deserialize_dependency_map",
        skip_serializing_if = "Option::is_none"
    )]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_dependency_map",
        skip_serializing_if = "Option::is_none"
    )]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_dependency_map",
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_dependency_map",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_record_map",
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,

    /// npm registry's per-version publisher metadata. When
    /// `trusted_publisher` is present alongside
    /// `dist.attestations.provenance`, the version was published
    /// through an OIDC-backed trusted-publisher integration *and*
    /// shipped a provenance attestation, which together count as the
    /// higher (`trustedPublisher`) trust rank, checked before falling
    /// back to the `provenance` attestation rank. The publisher flag
    /// without provenance is ignored.
    ///
    /// Carried on the wire as `_npmUser` (note the leading
    /// underscore).
    #[serde(
        default,
        rename = "_npmUser",
        deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent",
        skip_serializing_if = "Option::is_none",
        alias = "_npm_user"
    )]
    pub npm_user: Option<NpmUser>,

    /// `deprecated` field on a per-version manifest. When present the
    /// version has been marked deprecated on the registry and carries
    /// the maintainer-supplied reason. The resolver uses this for the
    /// deprecated-fallback in `pickVersionByVersionRange`: if the
    /// highest version satisfying the range is deprecated, retry the
    /// pick against the non-deprecated subset.
    ///
    /// **Wire format:** the field is nominally a string, but the real
    /// npm registry occasionally serves `"deprecated": false` for
    /// never-deprecated versions.
    #[serde(
        default,
        deserialize_with = "deserialize_deprecated_field",
        skip_serializing_if = "Option::is_none"
    )]
    pub deprecated: Option<String>,

    /// Every other field of the registry's per-version manifest, captured
    /// verbatim. The whole picked manifest is carried through, and the
    /// lockfile writer reads `engines` / `cpu` / `os` / `libc` / `bin` /
    /// `bundleDependencies` off it to populate the `packages:` entry. Keeping a
    /// flatten catch-all (rather than a typed field per key) preserves that
    /// passthrough and tolerates the historical shape variance npm serves.
    #[serde(flatten)]
    pub other: HashMap<String, serde_json::Value>,
}

impl Eq for PackageVersion {}

impl PartialEq for PackageVersion {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist
    }
}

impl PackageVersion {
    pub async fn fetch_from_registry(
        name: &str,
        tag: PackageTag,
        http_client: &ThrottledClient,
        registry: &str,
        auth_headers: &AuthHeaders,
    ) -> Result<Self, RegistryError> {
        // Format once and reuse for the request, the auth-header
        // lookup, and the error mapper. Keeps the auth lookup and
        // request URL byte-identical and saves two formats.
        let encoded_name = pnpm_network::encode_package_name(name);
        let normalized = normalize_registry_url(registry);
        let url = format!("{normalized}{encoded_name}/{}", tag.registry_path_segment());
        let network_error = |error| NetworkError { error, url: url.clone() };

        // Hold the semaphore permit across send + body consumption so the
        // socket-bound stays effective under concurrent fan-out. See the
        // doc comment on `ThrottledClientGuard`.
        let guard = http_client.acquire_for_url(&url).await;
        let mut request = guard
            .get(&url)
            .header(
                "accept",
                "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
            );
        if let Some(value) = auth_headers.for_url_with_package(&url, Some(name)) {
            request = request.header("authorization", value);
        }
        request
            .send()
            .await
            .map_err(network_error)?
            // See the same guard in `Package::fetch_from_registry`.
            .error_for_status()
            .map_err(network_error)?
            .json::<PackageVersion>()
            .await
            .map_err(network_error)?
            .pipe(Ok)
    }

    #[must_use]
    pub fn as_tarball_url(&self) -> &str {
        self.dist.tarball.as_str()
    }

    pub fn dependencies(
        &self,
        with_peer_dependencies: bool,
    ) -> impl Iterator<Item = (&'_ str, &'_ str)> {
        let dependencies = self.dependencies.iter().flatten();

        let peer_dependencies = with_peer_dependencies
            .then_some(&self.peer_dependencies)
            .into_iter()
            .flatten()
            .flatten();

        dependencies
            .chain(peer_dependencies)
            .map(|(name, version)| (name.as_str(), version.as_str()))
    }
}

mod wire;

#[cfg(test)]
mod tests;
