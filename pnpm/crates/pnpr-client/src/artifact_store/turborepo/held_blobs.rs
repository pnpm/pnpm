use super::super::super::{BTreeMap, PnprClientError, PublishArtifactRequest, VerifiedArtifact};
use std::{collections::HashMap, sync::Mutex};

/// The blobs of the artifacts a lookup selected, until the caller downloads
/// them. The Turborepo API has no blob route: an artifact carries its blobs.
///
/// Each blob is counted once per selected artifact that holds it and dropped
/// when every one has taken it, so a run that restores many artifacts holds
/// only those not yet restored.
#[derive(Default)]
pub(super) struct HeldBlobs(Mutex<HashMap<String, (Vec<u8>, usize)>>);

impl HeldBlobs {
    /// Hold the blobs of each selected artifact. An artifact whose blobs do
    /// not match its signed manifest is dropped from the selection.
    pub(super) fn hold(
        &self,
        selected: BTreeMap<String, VerifiedArtifact>,
        publications: &[PublishArtifactRequest],
    ) -> BTreeMap<String, VerifiedArtifact> {
        let mut held = self.0.lock().expect("blob map lock is not poisoned");
        selected
            .into_iter()
            .filter(|(_, artifact)| {
                let Some(publication) = publications
                    .iter()
                    .find(|request| request.envelope == artifact.envelope)
                    .and_then(|request| request.validate().ok())
                else {
                    return false;
                };
                for (integrity, bytes) in publication.blobs {
                    held.entry(integrity)
                        .or_insert((bytes, 0))
                        .1 += 1;
                }
                true
            })
            .collect()
    }

    /// One held blob. A blob is handed out once per artifact that holds it.
    pub(super) fn take(&self, integrity: &str) -> Result<Vec<u8>, PnprClientError> {
        let mut held = self.0.lock().expect("blob map lock is not poisoned");
        let Some((bytes, holders)) = held.get_mut(integrity) else {
            return Err(PnprClientError::Protocol(format!(
                "blob {integrity:?} is not part of a fetched artifact",
            )));
        };
        *holders -= 1;
        if *holders == 0 {
            return Ok(held.remove(integrity).expect("the blob is held").0);
        }
        Ok(bytes.clone())
    }
}
