use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::PackageVersion;
use crate::package_distribution::{AttestationsDist, PackageDistribution};

/// The fields of a version that the lockfile policy checks read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPolicyFields {
    pub dist: PackageDistribution,
    pub npm_user: Option<NpmUser>,
}

impl From<&PackageVersion> for VersionPolicyFields {
    fn from(version: &PackageVersion) -> Self {
        VersionPolicyFields { dist: version.dist.clone(), npm_user: version.npm_user.clone() }
    }
}

/// A [`PackageVersion`] without its catch-all [`PackageVersion::other`]
/// map, decoded for [`VersionPolicyFields`]. It declares every field
/// [`PackageVersion`] declares, since a duplicated declared key fails either
/// way, and gives each one that can fail the same deserializer. A fragment
/// decodes here exactly when it decodes as a [`PackageVersion`]. Dropping
/// the `#[serde(flatten)]` catch-all spares buffering and copying every
/// other key of the manifest.
#[derive(Deserialize)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "Mirrors the declared fields of `PackageVersion`; grouping them would need `#[serde(flatten)]`, which buffers the manifest.",
    )
)]
pub(crate) struct PolicyFieldsProbe {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "version")]
    _version: node_semver::Version,
    dist: PackageDistribution,
    #[serde(default, rename = "dependencies", deserialize_with = "deserialize_dependency_map")]
    _dependencies: Option<HashMap<String, String>>,
    #[serde(default, rename = "devDependencies", deserialize_with = "deserialize_dependency_map")]
    _dev_dependencies: Option<HashMap<String, String>>,
    #[serde(default, rename = "peerDependencies", deserialize_with = "deserialize_dependency_map")]
    _peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        rename = "optionalDependencies",
        deserialize_with = "deserialize_dependency_map"
    )]
    _optional_dependencies: Option<HashMap<String, String>>,
    /// Declared only so a duplicated key fails as it does on
    /// [`PackageVersion`]; its decoder there never fails, so it is skipped.
    #[serde(default, rename = "peerDependenciesMeta")]
    _peer_dependencies_meta: Option<serde::de::IgnoredAny>,
    #[serde(
        default,
        rename = "_npmUser",
        deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent",
        alias = "_npm_user"
    )]
    npm_user: Option<NpmUser>,
    #[serde(default, rename = "deprecated", deserialize_with = "deserialize_deprecated_field")]
    _deprecated: Option<String>,
}

impl From<PolicyFieldsProbe> for VersionPolicyFields {
    fn from(probe: PolicyFieldsProbe) -> Self {
        VersionPolicyFields { dist: probe.dist, npm_user: probe.npm_user }
    }
}

/// Deserialize a `Record<string, string>`-shaped dependency map while
/// tolerating historical npm registry entries whose values are objects
/// or other non-string shapes. Non-string entries are silently dropped
/// (e.g. `deep-diff@0.1.0`'s nested `devDependencies`). Missing field
/// and JSON `null` both decode to `None`; a present map (even one whose
/// entries are all dropped) decodes to `Some`.
pub(crate) fn deserialize_dependency_map<'de, Deser>(
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
pub(crate) fn deserialize_deprecated_field<'de, Deser>(
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
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDependencyMeta {
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_strict_flag",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional: Option<bool>,
}

/// `_npmUser` field on a per-version manifest. The verifier reads
/// `approver` and `trusted_publisher` to assign the trust rank
/// (`stagedPublish` > `trustedPublisher` > `provenance` > none).
/// `name` / `email` are kept for round-trip parity, and are decoded
/// leniently so neither can cost the version its trust rank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmUser {
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_text_or_absent",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_text_or_absent",
        skip_serializing_if = "Option::is_none"
    )]
    pub email: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_presence_marker",
        skip_serializing_if = "Option::is_none"
    )]
    pub approver: Option<Approver>,
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_presence_marker",
        skip_serializing_if = "Option::is_none"
    )]
    pub trusted_publisher: Option<TrustedPublisher>,
}

/// `_npmUser.approver` record on a per-version manifest. Its presence
/// marks a staged publish — one that required a 2FA publish approval,
/// the strongest trust signal. The verifier only checks for the
/// field's presence; `name` / `email` are kept for round-trip parity.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approver {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// OIDC trusted-publisher record on `_npmUser.trustedPublisher`.
/// The verifier only checks for the field's presence; the inner
/// values are kept for round-trip parity, and stay `None` for a
/// registry that marks the publisher without describing it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedPublisher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_config_id: Option<String>,
}

/// Fields read while comparing trust evidence across published versions.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionTrustMetadata {
    /// npm's publisher and approver markers.
    #[serde(
        default,
        rename = "_npmUser",
        alias = "_npm_user",
        deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent"
    )]
    pub npm_user: Option<NpmUser>,
    /// Distribution metadata containing provenance attestations.
    #[serde(default)]
    pub dist: Option<VersionTrustDist>,
}

/// Trust-relevant fields from a version's distribution metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionTrustDist {
    /// Provenance and its optional registry URL.
    #[serde(default, deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent")]
    pub attestations: Option<AttestationsDist>,
}

impl From<&PackageVersion> for VersionTrustMetadata {
    fn from(version: &PackageVersion) -> Self {
        VersionTrustMetadata {
            npm_user: version.npm_user.clone(),
            dist: Some(VersionTrustDist { attestations: version.dist.attestations.clone() }),
        }
    }
}
