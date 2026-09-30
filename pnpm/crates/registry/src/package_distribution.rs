use deser::{
    Deserialize, Serialize,
    adapters::{DisplayFromStr, FromInto},
};
use deser_value::Value;
use ssri::Integrity;

use crate::wire_tolerance::{AdvisoryCount, PresenceMarker, RecordOrAbsent, TextOrAbsent};

#[derive(Debug, Default, Clone, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct PackageDistribution {
    /// Subresource integrity for the version's tarball.
    ///
    /// Decoded strictly, unlike the advisory fields below: a registry
    /// package with no usable integrity cannot be locked at all — the
    /// snapshot builder rejects it — so softening the parse would only
    /// move "pnpm cannot verify this tarball" to a later, quieter
    /// failure. An unparsable value fails the manifest, and so the
    /// version, on purpose.
    #[deser(as = Option<DisplayFromStr>)]
    pub integrity: Option<Integrity>,
    pub shasum: Option<String>,
    pub tarball: String,
    #[deser(skip_serializing_if = Option::is_none)]
    pub revision: Option<Value>,
    #[deser(skip_serializing_if = Option::is_none)]
    pub revisions: Option<Value>,
    /// Number of files in the tarball, as the registry reports it.
    ///
    /// Advisory: any integral, non-negative encoding decodes — `12`,
    /// `12.0`, `"12"` — and anything else reads as "not reported".
    #[deser(default, deserialize_as = FromInto<AdvisoryCount>)]
    pub file_count: Option<usize>,
    /// Unpacked byte size of the tarball, as the registry reports it.
    /// Read only as an allocation hint by the tarball extractor, which
    /// caps it — never trusted as fact.
    ///
    /// Decoded as leniently as [`Self::file_count`].
    #[deser(default, deserialize_as = FromInto<AdvisoryCount>)]
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
    #[deser(
        default,
        deserialize_as = FromInto<RecordOrAbsent<AttestationsDist>>,
        skip_serializing_if = Option::is_none
    )]
    pub attestations: Option<AttestationsDist>,
}

/// Container for the attestation evidence a version exposes on its
/// `dist.attestations` field. Right now the only value the verifier
/// reads is `provenance`; the `url` field is the registry's link to
/// the raw Sigstore bundle and is kept for round-trip parity, decoded
/// leniently so it cannot cost the version its provenance rank.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct AttestationsDist {
    #[deser(
        default,
        deserialize_as = FromInto<PresenceMarker<ProvenanceMeta>>,
        skip_serializing_if = Option::is_none
    )]
    pub provenance: Option<ProvenanceMeta>,
    #[deser(
        default,
        deserialize_as = FromInto<TextOrAbsent>,
        skip_serializing_if = Option::is_none
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
#[deser(rename_all = "camelCase")]
pub struct ProvenanceMeta {
    #[deser(skip_serializing_if = Option::is_none)]
    pub predicate_type: Option<String>,
}

impl PartialEq for PackageDistribution {
    fn eq(&self, other: &Self) -> bool {
        self.integrity == other.integrity
    }
}

#[cfg(test)]
mod tests;
