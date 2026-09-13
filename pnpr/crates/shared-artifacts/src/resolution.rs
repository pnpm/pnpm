use super::{
    ArtifactCandidate, HashSet, MAX_CANDIDATES, MAX_VARIANTS_PER_CANDIDATE, RegistryError,
    ResolveArtifactsRequest, ResolveArtifactsResponse, ResolveBudget, ResolvedArtifact, Result,
    SharedArtifactStore, bad_request, entry_digest, is_variant_file, matching_variant, object_name,
    owner_key, protocol_error,
};
use futures_util::StreamExt as _;
impl SharedArtifactStore {
    pub async fn resolve(&self, username: &str, body: &[u8]) -> Result<ResolveArtifactsResponse> {
        let request: ResolveArtifactsRequest = serde_json::from_slice(body)
            .map_err(|err| bad_request(format!("invalid shared artifact lookup: {err}")))?;
        if request.candidates.len() > MAX_CANDIDATES {
            return Err(bad_request(format!(
                "lookup contains {} candidates; limit is {MAX_CANDIDATES}",
                request.candidates.len(),
            )));
        }
        let mut seen = HashSet::with_capacity(request.candidates.len());
        let mut artifacts = Vec::new();
        let mut budget = ResolveBudget {
            used_bytes: serde_json::to_vec(&ResolveArtifactsResponse {
                artifacts: Vec::new(),
            })?
            .len(),
        };
        for candidate in request.candidates {
            candidate
                .validate()
                .map_err(|err| protocol_error(&err))?;
            if !seen.insert(candidate.key.clone()) {
                return Err(bad_request(
                    "lookup contains a duplicate candidate".to_string(),
                ));
            }
            let Some(resolved) = self.resolve_candidate(username, &candidate, &mut budget)
                .await?
            else {
                continue;
            };
            budget.add_response(&resolved, !artifacts.is_empty())?;
            artifacts.push(resolved);
        }
        Ok(ResolveArtifactsResponse {
            artifacts,
        })
    }

    async fn resolve_candidate(
        &self,
        username: &str,
        candidate: &ArtifactCandidate,
        budget: &mut ResolveBudget,
    ) -> Result<Option<ResolvedArtifact>> {
        let owner = match owner_key(username, &candidate.owner) {
            Ok(owner) => owner,
            Err(RegistryError::Forbidden { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let entry = entry_digest(&candidate.key, &candidate.subject);
        let prefix = format!("{owner}/entries/{entry}/");
        let prefix = self.object_path(&prefix);
        let mut listing = self.store.list(Some(&prefix));
        let mut variants = Vec::new();
        let mut scanned_variants = 0;
        while scanned_variants < MAX_VARIANTS_PER_CANDIDATE {
            let Some(entry) = listing.next().await else {
                break;
            };
            let entry = entry?;
            if !is_variant_file(object_name(&entry.location)) {
                continue;
            }
            scanned_variants += 1;
            budget.add_scan(entry.size)?;
            let Some(bytes) = self.read_object_path(&entry.location).await? else {
                continue;
            };
            if let Some(variant) = matching_variant(&bytes, candidate) {
                variants.push(variant);
            }
        }
        Ok((!variants.is_empty()).then(|| ResolvedArtifact {
            key: candidate.key.clone(),
            variants,
        }))
    }
}
