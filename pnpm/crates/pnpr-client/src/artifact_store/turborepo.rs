//! A server that speaks the Turborepo Remote Cache API, used as an artifact
//! store.
//!
//! The API is a flat store of opaque objects addressed by hash
//! (`GET` and `PUT /v8/artifacts/<hash>`). The store mirrors pnpr's artifact
//! routes on it:
//!
//! - an artifact's signed envelope is stored once per compatibility scope it
//!   reaches, under a hash of the owner, the input key, and the scope, so a
//!   scope holds one artifact as on a pnpr server;
//! - each blob is stored once per owner, under a hash of the owner and its
//!   integrity, and checked against that integrity when downloaded.
//!
//! The API cannot create an object only when it is absent, so a later
//! publication replaces an earlier one. Only a machine that found nothing it
//! could restore publishes, and everything stored is verified when read, so
//! a replacement serves the machines its own build serves.

use super::super::{
    ARTIFACT_REQUEST_TIMEOUT, ArtifactBlobRequest, ArtifactCandidate, ArtifactPublication,
    BTreeMap, PnprClientError, ResolveArtifactsOptions, ResolveArtifactsResponse,
    SignedArtifactEnvelope,
    blob_transfer::{BlobBody, blob_body, blob_timeout, read_blob, write_blob},
    response_body_bounded,
};
use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use pnpm_shared_artifact_protocol::{
    ArtifactVariant, CompatibilityConstraints, CompatibilityScopes, MAX_ENCODED_SIGNATURE_SIZE,
    MAX_ENCODED_SIGNED_PAYLOAD_SIZE, OwnerScope, ResolvedArtifact, compatibility_scopes,
};
use reqwest::{Client, StatusCode, redirect::Policy};
use std::path::Path;

/// The scope of an artifact that applies to every machine.
const UNIVERSAL_SCOPE: &str = "universal";
/// An envelope's payload and signature, plus room for the JSON around them.
const MAX_ENVELOPE_BODY: usize =
    MAX_ENCODED_SIGNED_PAYLOAD_SIZE + MAX_ENCODED_SIGNATURE_SIZE + 64 * 1024;
/// Requests one lookup or publication has in flight at once.
const REQUEST_CONCURRENCY: usize = 16;

pub struct TurborepoArtifactStore {
    http: Client,
    base_url: String,
    /// `?teamId=…` or `?slug=…`, or empty.
    team_query: String,
    authorization: Option<String>,
}

impl TurborepoArtifactStore {
    /// A store at `url`, the API base the `/v8/artifacts` routes hang off.
    /// Credentials are refused for a URL that is neither HTTPS nor loopback.
    pub fn new(
        url: &str,
        team: Option<&str>,
        authorization: Option<String>,
    ) -> Result<TurborepoArtifactStore, PnprClientError> {
        if authorization.is_some() && !pnpm_network::is_url_secure_for_credentials(url) {
            return Err(PnprClientError::RemoteCache(
                "remote cache credentials require HTTPS or a loopback server".to_string(),
            ));
        }
        let http = Client::builder()
            .redirect(Policy::none())
            .timeout(ARTIFACT_REQUEST_TIMEOUT)
            .build()?;
        Ok(TurborepoArtifactStore {
            http,
            base_url: url.trim_end_matches('/').to_string(),
            team_query: team.map_or_else(String::new, team_query),
            authorization,
        })
    }

    pub(super) fn channel(&self) -> &str {
        &self.base_url
    }

    /// The stored envelopes of every candidate, in each scope this consumer
    /// falls in. A lookup that is missing, malformed, or fails is a miss.
    /// The lookup fails only when the server refuses the credentials or
    /// answers none of it.
    pub(super) async fn fetch_artifacts(
        &self,
        opts: &ResolveArtifactsOptions,
    ) -> Result<ResolveArtifactsResponse, PnprClientError> {
        let scopes = consumer_scopes(&opts.supported_tags);
        // Owned, so the stream's futures borrow nothing but `self`, which
        // keeps them `Send` for every caller.
        let lookups: Vec<(ArtifactCandidate, String)> = opts.candidates
            .iter()
            .flat_map(|candidate| {
                scopes
                    .iter()
                    .map(|scope| (candidate.clone(), scope.clone()))
            })
            .collect();
        let fetched: Vec<_> = stream::iter(lookups)
            .map(|(candidate, scope)| async move {
                let envelope = self.fetch_envelope(&candidate, &scope).await?;
                Ok(envelope.map(|envelope| (candidate.key, envelope)))
            })
            .buffer_unordered(REQUEST_CONCURRENCY)
            .collect()
            .await;
        let mut variants: BTreeMap<String, Vec<ArtifactVariant>> = BTreeMap::new();
        for (key, envelope) in successful_lookups(fetched)? {
            variants
                .entry(key)
                .or_default()
                .push(ArtifactVariant { envelope });
        }
        Ok(ResolveArtifactsResponse {
            artifacts: variants
                .into_iter()
                .map(|(key, variants)| ResolvedArtifact { key, variants })
                .collect(),
        })
    }

    async fn fetch_envelope(
        &self,
        candidate: &ArtifactCandidate,
        scope: &str,
    ) -> Result<Option<SignedArtifactEnvelope>, LookupFailure> {
        let url = self.artifact_url(&envelope_hash(&candidate.owner, &candidate.key, scope));
        let Some(body) = self.get(&url, MAX_ENVELOPE_BODY).await? else {
            return Ok(None);
        };
        Ok(serde_json::from_slice(&body).ok())
    }

    /// One blob of an artifact, held to `size` and checked against its
    /// integrity.
    pub(super) async fn download_blob(
        &self,
        request: &ArtifactBlobRequest,
        size: u64,
    ) -> Result<Vec<u8>, PnprClientError> {
        read_blob(self.blob_response(request, size).await?, &request.integrity, size).await
    }

    /// [`Self::download_blob`] into a new file at `destination`.
    pub(super) async fn download_blob_to(
        &self,
        request: &ArtifactBlobRequest,
        size: u64,
        destination: &Path,
    ) -> Result<(), PnprClientError> {
        let response = self.blob_response(request, size).await?;
        write_blob(response, &request.integrity, size, destination).await
    }

    async fn blob_response(
        &self,
        request: &ArtifactBlobRequest,
        size: u64,
    ) -> Result<reqwest::Response, PnprClientError> {
        request.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let url = self.artifact_url(&blob_hash(&request.owner, &request.integrity));
        let response = self
            .authorize(self.http.get(url))
            .timeout(blob_timeout(ARTIFACT_REQUEST_TIMEOUT, size))
            .send()
            .await?;
        match response.status() {
            StatusCode::NOT_FOUND => Err(PnprClientError::Protocol(format!(
                "the remote cache holds no blob {:?}",
                request.integrity,
            ))),
            status if status.is_success() => Ok(response),
            status => {
                Err(PnprClientError::Server(format!("GET {} returned {status}", self.base_url)))
            }
        }
    }

    /// Store the artifact's blobs, then its envelope in every scope it
    /// reaches, so an envelope is never stored ahead of its blobs.
    ///
    /// Every blob is uploaded, including one already stored: an object can be
    /// replaced, so one that fails verification is mended by the next
    /// publication that carries it.
    pub(super) async fn publish(
        &self,
        publication: &ArtifactPublication,
    ) -> Result<(), PnprClientError> {
        let payload = publication.validate()?;
        let owner = &payload.owner;
        // Owned, so the futures borrow nothing but `self` and stay `Send`
        // for every caller.
        let blobs: Vec<_> = publication.blobs
            .iter()
            .map(|blob| (self.artifact_url(&blob_hash(owner, &blob.integrity)), blob.clone()))
            .collect();
        stream::iter(blobs)
            .map(|(url, blob)| async move { self.put(&url, blob_body(&blob).await?, blob.size).await })
            .buffer_unordered(REQUEST_CONCURRENCY)
            .try_collect::<()>()
            .await?;
        let envelope = serde_json::to_vec(&publication.envelope)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let envelopes: Vec<_> = publication_scopes(&payload.compatibility)
            .iter()
            .map(|scope| self.artifact_url(&envelope_hash(owner, &publication.key, scope)))
            .collect();
        stream::iter(envelopes)
            .map(|url| {
                let envelope = envelope.clone();
                let size = envelope.len() as u64;
                async move { self.put(&url, envelope.into(), size).await }
            })
            .buffer_unordered(REQUEST_CONCURRENCY)
            .try_collect()
            .await
    }

    /// The body stored at `url`, or `None` when nothing is.
    async fn get(&self, url: &str, limit: usize) -> Result<Option<Vec<u8>>, LookupFailure> {
        let response = self
            .authorize(self.http.get(url))
            .send()
            .await
            .map_err(LookupFailure::failed)?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            let error = PnprClientError::Server(format!("GET {} returned {status}", self.base_url));
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                return Err(LookupFailure::refused(error));
            }
            return Err(LookupFailure::failed(error));
        }
        response_body_bounded(response, limit).await.map(Some).map_err(LookupFailure::failed)
    }

    async fn put(&self, url: &str, body: BlobBody, size: u64) -> Result<(), PnprClientError> {
        let response = self
            .authorize(self.http.put(url))
            .timeout(blob_timeout(ARTIFACT_REQUEST_TIMEOUT, size))
            .header("content-type", "application/octet-stream")
            .header("content-length", size)
            .body(body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(PnprClientError::Server(format!(
                "PUT {} returned {}",
                self.base_url,
                response.status(),
            )));
        }
        Ok(())
    }

    fn artifact_url(&self, hash: &str) -> String {
        format!("{}/v8/artifacts/{hash}{}", self.base_url, self.team_query)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.authorization {
            Some(authorization) => request.header("authorization", authorization),
            None => request,
        }
    }
}

/// Why one request of a lookup found nothing.
struct LookupFailure {
    error: PnprClientError,
    /// The server refused the credentials, which every other request would
    /// meet too.
    refused: bool,
}

impl LookupFailure {
    fn failed(error: impl Into<PnprClientError>) -> Self {
        LookupFailure { error: error.into(), refused: false }
    }

    fn refused(error: PnprClientError) -> Self {
        LookupFailure { error, refused: true }
    }
}

type FoundEnvelope = (String, SignedArtifactEnvelope);

/// The envelopes the lookups found. A failed lookup is a miss, unless the
/// server refused the credentials or every lookup failed.
fn successful_lookups(
    fetched: Vec<Result<Option<FoundEnvelope>, LookupFailure>>,
) -> Result<Vec<FoundEnvelope>, PnprClientError> {
    let lookups = fetched.len();
    let mut found = Vec::new();
    let mut failures = Vec::new();
    for lookup in fetched {
        match lookup {
            Ok(envelope) => found.extend(envelope),
            Err(failure) if failure.refused => return Err(failure.error),
            Err(failure) => failures.push(failure.error),
        }
    }
    if lookups > 0 && failures.len() == lookups {
        return Err(failures.swap_remove(0));
    }
    for error in failures {
        tracing::debug!(target: "pacquet::artifacts", %error, "remote cache lookup failed");
    }
    Ok(found)
}

/// The hash an envelope is stored under: a function of the owner, the input
/// key, and the compatibility scope, so a consumer can name it without
/// listing anything.
fn envelope_hash(owner: &OwnerScope, input_key: &str, scope: &str) -> String {
    pnpm_crypto_hash::create_hex_hash(&format!(
        "pnpm-shared-artifact:v1\0{}\0{input_key}\0{scope}",
        owner.namespace(),
    ))
}

/// The hash a blob is stored under, within its owner's namespace as on pnpr.
fn blob_hash(owner: &OwnerScope, integrity: &str) -> String {
    pnpm_crypto_hash::create_hex_hash(&format!(
        "pnpm-shared-artifact-blob:v1\0{}\0{integrity}",
        owner.namespace(),
    ))
}

/// The scopes a consumer with these supported tags falls in, which always
/// include the one universal artifacts are stored in.
fn consumer_scopes(supported_tags: &[String]) -> Vec<String> {
    let tagged = CompatibilityConstraints::Tagged { tags: supported_tags.to_vec() };
    let mut scopes = match compatibility_scopes(&tagged) {
        CompatibilityScopes::These(scopes) => scopes.into_iter().collect(),
        CompatibilityScopes::Every => Vec::new(),
    };
    scopes.push(UNIVERSAL_SCOPE.to_string());
    scopes
}

fn publication_scopes(compatibility: &CompatibilityConstraints) -> Vec<String> {
    match compatibility_scopes(compatibility) {
        CompatibilityScopes::Every => vec![UNIVERSAL_SCOPE.to_string()],
        CompatibilityScopes::These(scopes) => scopes.into_iter().collect(),
    }
}

/// Turborepo's convention: a Vercel team id starts with `team_`, anything
/// else is a team slug.
fn team_query(team: &str) -> String {
    let name = if team.starts_with("team_") { "teamId" } else { "slug" };
    let query =
        url::form_urlencoded::Serializer::new(String::new()).append_pair(name, team).finish();
    format!("?{query}")
}
