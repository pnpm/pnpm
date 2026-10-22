use std::collections::BTreeMap;

use pnpm_package_manifest::PackageBin;
use serde::{Deserialize, Serialize};
use ssri::Integrity;

#[derive(Debug, Default, Clone, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror npm registry dist metadata."
    )
)]
pub struct PackageDistribution {
    /// Subresource integrity for the version's tarball.
    ///
    /// Decoded strictly, unlike the advisory fields below: a registry
    /// package with no usable integrity cannot be locked at all — the
    /// snapshot builder rejects it — so softening the parse would only
    /// move "pnpm cannot verify this tarball" to a later, quieter
    /// failure. An unparsable value fails the manifest, and so the
    /// version, on purpose.
    pub integrity: Option<Integrity>,
    pub shasum: Option<String>,
    /// The tarball URL, or empty when the registry names none. A registry
    /// with a declared URL layout may omit it, and the resolver then
    /// rebuilds it from the package's identity.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tarball: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisions: Option<serde_json::Value>,
    /// Number of files in the tarball, as the registry reports it.
    ///
    /// Advisory: any integral, non-negative encoding decodes — `12`,
    /// `12.0`, `"12"` — and anything else reads as "not reported".
    #[serde(default, deserialize_with = "crate::wire_tolerance::deserialize_advisory_count")]
    pub file_count: Option<usize>,
    /// Unpacked byte size of the tarball, as the registry reports it.
    /// Read only as an allocation hint by the tarball extractor, which
    /// caps it — never trusted as fact.
    ///
    /// Decoded as leniently as [`Self::file_count`].
    #[serde(default, deserialize_with = "crate::wire_tolerance::deserialize_advisory_count")]
    pub unpacked_size: Option<usize>,

    /// Sigstore-based supply-chain evidence the npm registry attaches
    /// to a published version. When `provenance` is present the
    /// version was published with a Sigstore attestation linking it
    /// to its source repo and CI run; `url` points at the
    /// `/-/npm/v1/attestations/<name>@<version>` endpoint that serves
    /// the raw bundle.
    ///
    /// Read by the `trustPolicy='no-downgrade'` verifier when it
    /// decides whether a version's trust evidence is weaker than
    /// an earlier-published one's.
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent",
        skip_serializing_if = "Option::is_none"
    )]
    pub attestations: Option<AttestationsDist>,

    /// Tarball-verified facts `pnpr` computes from the archive's own
    /// bytes and attaches under `dist.pnpr`.
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_record_or_absent",
        skip_serializing_if = "Option::is_none"
    )]
    pub pnpr: Option<PnprDistribution>,
}

/// Tarball-verified facts `pnpr` attaches to `dist.pnpr`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PnprDistribution {
    /// The build-trigger verdict of `pnpm_tarball::tar_requires_build`.
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_strict_flag",
        skip_serializing_if = "Option::is_none"
    )]
    pub requires_build: Option<bool>,
    /// Precalculated executable bin entries and their shebang runtimes.
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_pnpr_bins",
        skip_serializing_if = "Option::is_none"
    )]
    pub bins: Option<BTreeMap<String, PackageBin>>,
}

/// Container for the attestation evidence a version exposes on its
/// `dist.attestations` field. Right now the only value the verifier
/// reads is `provenance`; the `url` field is the registry's link to
/// the raw Sigstore bundle and is kept for round-trip parity, decoded
/// leniently so it cannot cost the version its provenance rank.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationsDist {
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_presence_marker",
        skip_serializing_if = "Option::is_none"
    )]
    pub provenance: Option<ProvenanceMeta>,
    #[serde(
        default,
        deserialize_with = "crate::wire_tolerance::deserialize_text_or_absent",
        skip_serializing_if = "Option::is_none"
    )]
    pub url: Option<String>,
}

/// Provenance attestation marker. The presence of this object on
/// `dist.attestations.provenance` is what counts as the "provenance"
/// rank when ranking a version's trust evidence; the inner
/// `predicateType` field is kept for round-trip parity but the
/// verifier itself does not inspect it. Because only the presence of
/// the field is read, a registry that marks provenance with something
/// other than an object still counts as carrying it, and decodes here
/// with no `predicateType`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate_type: Option<String>,
}

impl PartialEq for PackageDistribution {
    fn eq(&self, other: &Self) -> bool {
        self.integrity == other.integrity
    }
}

#[cfg(test)]
mod tests;
