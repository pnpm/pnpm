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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,

    /// npm registry's per-version publisher metadata. When
    /// `trusted_publisher` is present alongside
    /// `dist.attestations.provenance`, the version was published
    /// through an OIDC-backed trusted-publisher integration *and*
    /// shipped a provenance attestation, which together count as the
    /// higher (`trustedPublisher`) trust rank that upstream's
    /// [`getTrustEvidence`](https://github.com/pnpm/pnpm/blob/fea5fd41da/resolving/npm-resolver/src/trustChecks.ts#L119-L127)
    /// checks before falling back to the `provenance` attestation
    /// rank. The publisher flag without provenance is ignored.
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
    /// **Wire format:** the field is declared as a string upstream
    /// (`PackageInRegistry.deprecated?: string`) but the real npm
    /// registry occasionally serves `"deprecated": false` for
    /// never-deprecated versions — JavaScript stores the boolean and
    /// the upstream `if (info.deprecated)` truthiness check happens
    /// to handle both shapes silently. Rust serde is strict, so we
    /// route through a custom deserializer that normalizes the field
    /// to `Option<String>`: a string stays a string, `false` becomes
    /// `None`, `true` becomes `Some("")` (deprecated without a
    /// recorded reason). Mirrors pnpm's
    /// [`PackageInRegistry.deprecated`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/packages/types/src/package.ts).
    #[serde(
        default,
        deserialize_with = "deserialize_deprecated_field",
        skip_serializing_if = "Option::is_none"
    )]
    pub deprecated: Option<String>,
}

/// Deserialize a `Record<string, string>`-shaped dependency map while
/// tolerating historical npm registry entries whose values are objects
/// or other non-string shapes. Non-string entries are silently dropped,
/// matching pnpm's JavaScript path which never validates the value
/// shape and relies on later `typeof spec === 'string'` checks to
/// ignore the bad rows (e.g. `deep-diff@0.1.0`'s nested
/// `devDependencies`). Missing field and JSON `null` both decode to
/// `None`; a present map (even one whose entries are all dropped)
/// decodes to `Some`.
fn deserialize_dependency_map<'de, Deser>(
    deserializer: Deser,
) -> Result<Option<HashMap<String, String>>, Deser::Error>
where
    Deser: serde::Deserializer<'de>,
{
    use serde::de::{self, MapAccess, Visitor};
    use std::fmt;

    struct DependencyMapVisitor;
    impl<'de> Visitor<'de> for DependencyMapVisitor {
        type Value = Option<HashMap<String, String>>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map of dependency name to version-spec string, or null")
        }
        fn visit_none<Err: de::Error>(self) -> Result<Self::Value, Err> {
            Ok(None)
        }
        fn visit_unit<Err: de::Error>(self) -> Result<Self::Value, Err> {
            Ok(None)
        }
        fn visit_some<Nested: serde::Deserializer<'de>>(
            self,
            deserializer: Nested,
        ) -> Result<Self::Value, Nested::Error> {
            deserializer.deserialize_any(DependencyMapVisitor)
        }
        fn visit_map<Map: MapAccess<'de>>(self, mut map: Map) -> Result<Self::Value, Map::Error> {
            let mut out = HashMap::new();
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value::<serde_json::Value>()?;
                if let serde_json::Value::String(spec) = value {
                    out.insert(key, spec);
                }
            }
            Ok(Some(out))
        }
    }
    deserializer.deserialize_any(DependencyMapVisitor)
}

/// Accept either a string or a boolean for the `deprecated` field.
/// A bool `true` becomes `Some("")`, a bool `false` becomes `None`;
/// a string stays as `Some(s)`. Missing field defaults to `None` via
/// the `#[serde(default)]` on the field itself.
fn deserialize_deprecated_field<'de, Deser>(
    deserializer: Deser,
) -> Result<Option<String>, Deser::Error>
where
    Deser: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct DeprecatedVisitor;
    impl<'de> Visitor<'de> for DeprecatedVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a deprecation reason (string), a boolean, or null")
        }
        fn visit_str<Err: de::Error>(self, value: &str) -> Result<Self::Value, Err> {
            Ok(Some(value.to_string()))
        }
        fn visit_string<Err: de::Error>(self, value: String) -> Result<Self::Value, Err> {
            Ok(Some(value))
        }
        fn visit_bool<Err: de::Error>(self, value: bool) -> Result<Self::Value, Err> {
            Ok(value.then(String::new))
        }
        fn visit_none<Err: de::Error>(self) -> Result<Self::Value, Err> {
            Ok(None)
        }
        fn visit_unit<Err: de::Error>(self) -> Result<Self::Value, Err> {
            Ok(None)
        }
        fn visit_some<Nested: serde::Deserializer<'de>>(
            self,
            deserializer: Nested,
        ) -> Result<Self::Value, Nested::Error> {
            deserializer.deserialize_any(DeprecatedVisitor)
        }
    }
    deserializer.deserialize_any(DeprecatedVisitor)
}

/// `peerDependenciesMeta[name]` shape from the npm registry. Only the
/// `optional` flag is consumed by the resolver; other fields the
/// registry may serve are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDependencyMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
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

    /// The abbreviated registry response (`application/vnd.npm.install-v1+json`)
    /// carries `optionalDependencies` and `peerDependenciesMeta` for any
    /// package that publishes them. Both must round-trip through
    /// [`PackageVersion`] so the resolver's `extract_children` reads the
    /// optional-dep edges and `extract_peer_dependencies` reads the
    /// per-peer `optional` flag. Dropping either field silently treats
    /// optional peers as required (auto-installed via
    /// `autoInstallPeers`) and skips `optionalDependencies` entirely.
    #[test]
    fn deserializes_optional_dependencies_and_peer_dependencies_meta() {
        let body = r#"{
            "name": "unstorage",
            "version": "1.17.5",
            "dist": {
                "integrity": "sha512-AAAA",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry.test/unstorage-1.17.5.tgz"
            },
            "peerDependencies": {
                "@vercel/kv": "^1 || ^2 || ^3",
                "ioredis": "^5.4.2"
            },
            "peerDependenciesMeta": {
                "@vercel/kv": { "optional": true },
                "ioredis": { "optional": true }
            },
            "optionalDependencies": {
                "sharp": "^0.34.0"
            }
        }"#;

        let pkg: PackageVersion =
            serde_json::from_str(body).expect("deserialize PackageVersion fixture");

        let optional = pkg.optional_dependencies.as_ref().expect("optionalDependencies present");
        assert_eq!(optional.get("sharp").map(String::as_str), Some("^0.34.0"));

        let peer_meta = pkg.peer_dependencies_meta.as_ref().expect("peerDependenciesMeta present");
        assert_eq!(peer_meta["@vercel/kv"].optional, Some(true));
        assert_eq!(peer_meta["ioredis"].optional, Some(true));

        // The JSON shape `serde_json::to_value(pkg)` produces feeds
        // `extract_children` / `extract_peer_dependencies` downstream;
        // both consume the camelCase keys verbatim.
        let value = serde_json::to_value(&pkg).expect("serialize PackageVersion");
        assert!(value.get("optionalDependencies").is_some_and(|v| v.is_object()));
        assert!(value.get("peerDependenciesMeta").is_some_and(|v| v.is_object()));
    }
}
