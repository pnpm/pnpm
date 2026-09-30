use std::collections::HashMap;

use deser::{
    Deserialize, Serialize,
    adapters::{DisplayFromStr, FromInto, MapSkipError},
};
use deser_value::Value;
use pipe_trait::Pipe;
use pnpm_network::{AuthHeaders, ThrottledClient};

use crate::{
    NetworkError, PackageTag, RegistryError, json,
    package_distribution::PackageDistribution,
    wire_tolerance::{
        DeprecationReason, PresenceMarker, RecordMap, RecordOrAbsent, StrictFlag, TextOrAbsent,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror npm registry version metadata."
    )
)]
pub struct PackageVersion {
    pub name: String,
    #[deser(as = DisplayFromStr)]
    pub version: node_semver::Version,
    pub dist: PackageDistribution,
    /// A `Record<string, string>`-shaped dependency map. Historical npm
    /// registry entries whose values are objects or other non-string
    /// shapes are dropped (e.g. `deep-diff@0.1.0`'s nested
    /// `devDependencies`). A missing field and JSON `null` both decode to
    /// `None`; a present map (even one whose entries are all dropped)
    /// decodes to `Some`.
    #[deser(as = Option<MapSkipError>, skip_serializing_if = Option::is_none)]
    pub dependencies: Option<HashMap<String, String>>,
    /// See [`Self::dependencies`].
    #[deser(as = Option<MapSkipError>, skip_serializing_if = Option::is_none)]
    pub dev_dependencies: Option<HashMap<String, String>>,
    /// See [`Self::dependencies`].
    #[deser(as = Option<MapSkipError>, skip_serializing_if = Option::is_none)]
    pub peer_dependencies: Option<HashMap<String, String>>,
    /// See [`Self::dependencies`].
    #[deser(as = Option<MapSkipError>, skip_serializing_if = Option::is_none)]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[deser(
        default,
        deserialize_as = FromInto<RecordMap<PeerDependencyMeta>>,
        skip_serializing_if = Option::is_none
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
    #[deser(
        default,
        rename = "_npmUser",
        alias = "_npm_user",
        deserialize_as = FromInto<RecordOrAbsent<NpmUser>>,
        skip_serializing_if = Option::is_none
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
    /// never-deprecated versions. A `false` boolean decodes as absent and
    /// a `true` one as a deprecation without a reason.
    #[deser(
        default,
        deserialize_as = FromInto<DeprecationReason>,
        skip_serializing_if = Option::is_none
    )]
    pub deprecated: Option<String>,

    /// Every other field of the registry's per-version manifest, captured
    /// verbatim. The whole picked manifest is carried through, and the
    /// lockfile writer reads `engines` / `cpu` / `os` / `libc` / `bin` /
    /// `bundleDependencies` off it to populate the `packages:` entry. Keeping a
    /// flatten catch-all (rather than a typed field per key) preserves that
    /// passthrough and tolerates the historical shape variance npm serves.
    #[deser(flatten)]
    pub other: HashMap<String, Value>,
}

impl Eq for PackageVersion {}

/// `peerDependenciesMeta[name]` shape from the npm registry. Only the
/// `optional` flag is consumed by the resolver; other fields the
/// registry may serve are ignored.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct PeerDependencyMeta {
    #[deser(default, deserialize_as = FromInto<StrictFlag>, skip_serializing_if = Option::is_none)]
    pub optional: Option<bool>,
}

/// `_npmUser` field on a per-version manifest. The verifier reads
/// `approver` and `trusted_publisher` to assign the trust rank
/// (`stagedPublish` > `trustedPublisher` > `provenance` > none).
/// `name` / `email` are kept for round-trip parity, and are decoded
/// leniently so neither can cost the version its trust rank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct NpmUser {
    #[deser(default, deserialize_as = FromInto<TextOrAbsent>, skip_serializing_if = Option::is_none)]
    pub name: Option<String>,
    #[deser(default, deserialize_as = FromInto<TextOrAbsent>, skip_serializing_if = Option::is_none)]
    pub email: Option<String>,
    #[deser(
        default,
        deserialize_as = FromInto<PresenceMarker<Approver>>,
        skip_serializing_if = Option::is_none
    )]
    pub approver: Option<Approver>,
    #[deser(
        default,
        deserialize_as = FromInto<PresenceMarker<TrustedPublisher>>,
        skip_serializing_if = Option::is_none
    )]
    pub trusted_publisher: Option<TrustedPublisher>,
}

/// `_npmUser.approver` record on a per-version manifest. Its presence
/// marks a staged publish — one that required a 2FA publish approval,
/// the strongest trust signal. The verifier only checks for the
/// field's presence; `name` / `email` are kept for round-trip parity.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct Approver {
    #[deser(skip_serializing_if = Option::is_none)]
    pub name: Option<String>,
    #[deser(skip_serializing_if = Option::is_none)]
    pub email: Option<String>,
}

/// OIDC trusted-publisher record on `_npmUser.trustedPublisher`.
/// The verifier only checks for the field's presence; the inner
/// values are kept for round-trip parity, and stay `None` for a
/// registry that marks the publisher without describing it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct TrustedPublisher {
    #[deser(skip_serializing_if = Option::is_none)]
    pub id: Option<String>,
    #[deser(skip_serializing_if = Option::is_none)]
    pub oidc_config_id: Option<String>,
}

impl PartialEq for PackageVersion {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist
    }
}

impl PackageVersion {
    /// Decode a version manifest.
    pub fn from_json(json: &str) -> Result<Self, deser::Error> {
        json::from_str(json)
    }

    /// The manifest as the `serde_json` tree the resolver hands on as the
    /// picked package's manifest.
    pub fn to_json_value(&self) -> Result<serde_json::Value, deser::Error> {
        deser_value::to_value(self).map(|value| json::to_serde_json(&value))
    }

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
        let url = format!("{registry}{encoded_name}/{}", tag.registry_path_segment());
        let network_error =
            |error: reqwest::Error| NetworkError { error: error.into(), url: url.clone() };

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
            .text()
            .await
            .map_err(network_error)?
            .pipe(|body| PackageVersion::from_json(&body))
            .map_err(|error| NetworkError { error: error.into(), url: url.clone() }.into())
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

#[cfg(test)]
mod tests;
