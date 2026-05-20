use std::collections::HashMap;

use pacquet_network::{AuthHeaders, ThrottledClient};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};

use crate::{NetworkError, PackageTag, RegistryError, package_distribution::PackageDistribution};

#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageVersion {
    pub name: String,
    pub version: node_semver::Version,
    pub dist: PackageDistribution,
    pub dependencies: Option<HashMap<String, String>>,
    pub dev_dependencies: Option<HashMap<String, String>>,
    pub peer_dependencies: Option<HashMap<String, String>>,

    /// npm registry's per-version publisher metadata. When
    /// `trusted_publisher` is present the version was published
    /// through an OIDC-backed trusted-publisher integration, which
    /// counts as the higher (`trustedPublisher`) trust rank that
    /// upstream's [`getTrustEvidence`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts#L119-L127)
    /// checks before falling back to the `provenance` attestation
    /// rank.
    ///
    /// Mirrors pnpm's
    /// [`PackageInRegistry._npmUser`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L29-L36)
    /// (note the leading underscore on the wire).
    #[serde(
        default,
        rename = "_npmUser",
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
    /// Mirrors pnpm's
    /// [`PackageInRegistry.deprecated`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/packages/types/src/package.ts)
    /// — exposed via the abbreviated metadata document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

/// `_npmUser` field on a per-version manifest. The verifier reads
/// `trusted_publisher` to assign the higher of the two trust ranks
/// (`trustedPublisher` > `provenance` > none). `name` / `email` are
/// kept for round-trip parity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmUser {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_publisher: Option<TrustedPublisher>,
}

/// OIDC trusted-publisher record on `_npmUser.trustedPublisher`.
/// The verifier only checks for the field's presence; the inner
/// values are kept for round-trip parity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedPublisher {
    pub id: String,
    pub oidc_config_id: String,
}

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
        let url = format!("{registry}{name}/{tag}");
        let network_error = |error| NetworkError { error, url: url.clone() };

        let mut request = http_client.acquire_for_url(&url).await.get(&url).header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        );
        // Same auth flow as `Package::fetch_from_registry`. See the
        // doc comment there.
        if let Some(value) = auth_headers.for_url(&url) {
            request = request.header("authorization", value);
        }
        request
            .send()
            .await
            .map_err(network_error)?
            .json::<PackageVersion>()
            .await
            .map_err(network_error)?
            .pipe(Ok)
    }

    pub fn to_virtual_store_name(&self) -> String {
        format!("{0}@{1}", self.name.replace('/', "+"), self.version)
    }

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

    pub fn serialize(&self, save_exact: bool) -> String {
        let prefix = if save_exact { "" } else { "^" };
        format!("{0}{1}", prefix, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthHeaders, PackageTag, PackageVersion, ThrottledClient};

    /// [`PackageVersion::fetch_from_registry`] must attach the
    /// registry-keyed `Authorization` header on every tag GET, just
    /// like [`crate::Package::fetch_from_registry`].
    #[tokio::test]
    async fn fetch_from_registry_attaches_authorization_header() {
        let mut server = mockito::Server::new_async().await;
        let body = r#"{
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAA",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry.test/acme-1.0.0.tgz"
            }
        }"#;
        let mock = server
            .mock("GET", "/acme/latest")
            .match_header("authorization", "Bearer top-secret")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect(1)
            .create_async()
            .await;

        let registry = format!("{}/", server.url());
        let client = ThrottledClient::default();
        let auth_headers = AuthHeaders::from_creds_map(
            [(pacquet_network::nerf_dart(&registry), "Bearer top-secret".to_owned())],
            None,
        );

        let pkg_version = PackageVersion::fetch_from_registry(
            "acme",
            PackageTag::Latest,
            &client,
            &registry,
            &auth_headers,
        )
        .await
        .expect("server should accept the request once the bearer header is attached");
        assert_eq!(pkg_version.name, "acme");
        mock.assert_async().await;
    }
}
