//! [`PackageVersions`] reads that decode one part of each version and
//! leave the rest of its manifest unhydrated.

use std::sync::{Arc, OnceLock};

use serde::Deserialize;

use super::{FragmentSource, PackageVersions, VersionSlot};
use crate::{PackageDistribution, VersionTrustMetadata};

/// Single-field view of a version manifest for [`PackageVersions::dist`].
#[derive(Deserialize)]
struct DistProbe {
    dist: PackageDistribution,
}

impl PackageVersions {
    /// `version`'s `dist`, decoded without hydrating or caching the rest
    /// of its manifest. `None` when the version is absent or its `dist`
    /// fails to decode.
    #[must_use]
    pub fn dist(&self, version: &str) -> Option<PackageDistribution> {
        let slot = self.slot(version)?;
        if let Some(parsed) = slot.parsed.get() {
            return parsed.as_ref().map(|manifest| manifest.dist.clone());
        }
        let Some(json) = slot.source.json() else {
            slot.report_undecodable(version, &self.corrupt_mirror_fragment);
            return None;
        };
        match serde_json::from_str::<DistProbe>(&json) {
            Ok(probe) => Some(probe.dist),
            Err(error) => {
                tracing::warn!(
                    target: "pnpm_registry",
                    %error,
                    version,
                    "skipping registry version with an undecodable dist",
                );
                slot.report_decode_error(version, &json, &self.corrupt_mirror_fragment);
                None
            }
        }
    }

    /// Copy that answers only [`Self::trust_metadata`], with each version's
    /// [`VersionTrustMetadata::evidence_only`] and nothing else of it
    /// decoded. A version whose trust metadata fails to decode stays
    /// listed and answers `None`. [`Self::get`] answers `None` for every
    /// version of the copy.
    #[must_use]
    pub fn trust_projection(&self) -> PackageVersions {
        let no_evidence = Arc::new(VersionTrustMetadata { npm_user: None, dist: None });
        PackageVersions {
            slots: self.slots
                .iter()
                .map(|(version, slot)| {
                    let trust = self
                        .slot_trust_metadata(version, slot)
                        .map(|trust| {
                            let evidence = trust.evidence_only();
                            if evidence.is_empty() {
                                Arc::clone(&no_evidence)
                            } else {
                                Arc::new(evidence)
                            }
                        });
                    let projected = VersionSlot {
                        source: FragmentSource::None,
                        parsed: OnceLock::from(None),
                        deprecated: OnceLock::new(),
                        trust: OnceLock::from(trust),
                    };
                    (version.clone(), projected)
                })
                .collect(),
            corrupt_mirror_fragment: Arc::clone(&self.corrupt_mirror_fragment),
        }
    }
}
