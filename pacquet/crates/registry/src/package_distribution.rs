use serde::{Deserialize, Serialize};
use ssri::Integrity;

#[derive(Debug, Default, Clone, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDistribution {
    pub integrity: Option<Integrity>,
    pub shasum: Option<String>,
    pub tarball: String,
    pub file_count: Option<usize>,
    pub unpacked_size: Option<usize>,

    /// Sigstore-based supply-chain evidence the npm registry attaches
    /// to a published version. When `provenance` is present the
    /// version was published with a Sigstore attestation linking it
    /// to its source repo and CI run; `url` points at the
    /// `/-/npm/v1/attestations/<name>@<version>` endpoint that serves
    /// the raw bundle.
    ///
    /// Mirrors pnpm's
    /// [`PackageInRegistry.dist.attestations`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L52-L57).
    /// Read by the `trustPolicy='no-downgrade'` verifier when it
    /// decides whether a version's trust evidence is weaker than
    /// an earlier-published one's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestations: Option<AttestationsDist>,
}

/// Container for the attestation evidence a version exposes on its
/// `dist.attestations` field. Right now the only value the verifier
/// reads is `provenance`; the `url` field is the registry's link to
/// the raw Sigstore bundle and is kept for round-trip parity.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationsDist {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Provenance attestation marker. The presence of this object on
/// `dist.attestations.provenance` is what counts as the "provenance"
/// rank for [`getTrustEvidence`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts#L119-L127);
/// the inner `predicateType` field is kept for round-trip parity but
/// the verifier itself does not inspect it.
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
