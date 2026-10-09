//! A server that speaks the Turborepo Remote Cache API, used as an artifact
//! store.
//!
//! The API is a flat store of opaque artifacts addressed by hash
//! (`GET`, `HEAD`, and `PUT /v8/artifacts/<hash>`). One stored artifact is a
//! [`PublishArtifactRequest`] as JSON: the signed envelope and its blobs. It
//! is stored once per compatibility scope the envelope reaches, under a hash
//! of the owner, the input key, and the scope. The first publication of a
//! scope wins, which is the one-artifact-per-scope rule a pnpr server
//! enforces too.

use super::super::{
    ARTIFACT_REQUEST_TIMEOUT, ArtifactBlobRequest, ArtifactCandidate, BTreeMap, PnprClientError,
    PublishArtifactRequest, ResolveArtifactsOptions, ResolveArtifactsResponse,
    response_body_bounded,
};
use futures_util::{StreamExt as _, stream};
use pnpm_shared_artifact_protocol::{
    ArtifactVariant, CompatibilityConstraints, CompatibilityScopes, MAX_ARTIFACT_SIZE,
    MAX_ENCODED_SIGNED_PAYLOAD_SIZE, OwnerScope, ResolvedArtifact, compatibility_scopes,
};
use reqwest::{Client, StatusCode, redirect::Policy};
use std::{collections::HashMap, sync::Mutex};

/// The scope of an artifact that applies to every machine.
const UNIVERSAL_SCOPE: &str = "universal";
/// Base64 inflates the blobs by a third; the rest is the envelope and JSON.
const MAX_ARTIFACT_BODY: usize =
    (MAX_ARTIFACT_SIZE as usize).div_ceil(3) * 4 + MAX_ENCODED_SIGNED_PAYLOAD_SIZE + 1024 * 1024;
const LOOKUP_CONCURRENCY: usize = 16;

pub struct TurborepoArtifactStore {
    http: Client,
    base_url: String,
    /// `?teamId=…` or `?slug=…`, or empty.
    team_query: String,
    authorization: Option<String>,
    /// The blobs of every artifact [`Self::fetch_artifacts`] received, by
    /// integrity. The API has no blob route: an artifact carries its blobs.
    fetched_blobs: Mutex<HashMap<String, Vec<u8>>>,
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
            return Err(PnprClientError::Protocol(
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
            fetched_blobs: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn channel(&self) -> &str {
        &self.base_url
    }

    /// The stored artifacts of every candidate, in each scope this consumer
    /// falls in. An artifact that is missing or malformed is a miss; a server
    /// that cannot be asked is an error.
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
            .map(|(candidate, scope)| async move { self.fetch(&candidate, &scope).await })
            .buffer_unordered(LOOKUP_CONCURRENCY)
            .collect()
            .await;
        let mut variants: BTreeMap<String, Vec<ArtifactVariant>> = BTreeMap::new();
        for fetched in fetched {
            let Some((key, request)) = fetched? else { continue };
            let Ok(publication) = request.validate() else { continue };
            self.fetched_blobs
                .lock()
                .expect("blob map lock is not poisoned")
                .extend(publication.blobs);
            variants
                .entry(key)
                .or_default()
                .push(ArtifactVariant { envelope: request.envelope });
        }
        Ok(ResolveArtifactsResponse {
            artifacts: variants
                .into_iter()
                .map(|(key, variants)| ResolvedArtifact { key, variants })
                .collect(),
        })
    }

    async fn fetch(
        &self,
        candidate: &ArtifactCandidate,
        scope: &str,
    ) -> Result<Option<(String, PublishArtifactRequest)>, PnprClientError> {
        let url = self.artifact_url(&candidate.owner, &candidate.key, scope);
        let response = self
            .authorize(self.http.get(&url))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(PnprClientError::Server(format!(
                "GET {} returned {}",
                self.base_url,
                response.status(),
            )));
        }
        let body = response_body_bounded(response, MAX_ARTIFACT_BODY).await?;
        Ok(serde_json::from_slice::<PublishArtifactRequest>(&body)
            .ok()
            .filter(|request| request.key == candidate.key)
            .map(|request| (candidate.key.clone(), request)))
    }

    pub(super) fn fetched_blob(
        &self,
        request: &ArtifactBlobRequest,
    ) -> Result<Vec<u8>, PnprClientError> {
        request.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        self.fetched_blobs
            .lock()
            .expect("blob map lock is not poisoned")
            .get(&request.integrity)
            .cloned()
            .ok_or_else(|| {
                PnprClientError::Protocol(format!(
                    "blob {:?} is not part of a fetched artifact",
                    request.integrity,
                ))
            })
    }

    /// Store the artifact in every scope it reaches that holds none yet.
    pub(super) async fn publish(
        &self,
        request: &PublishArtifactRequest,
    ) -> Result<(), PnprClientError> {
        let publication =
            request.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let body =
            serde_json::to_vec(request).map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        for scope in publication_scopes(&publication.payload.compatibility) {
            let url = self.artifact_url(&publication.payload.owner, &request.key, &scope);
            if self.exists(&url).await? {
                continue;
            }
            let response = self
                .authorize(self.http.put(&url))
                .header("content-type", "application/octet-stream")
                .body(body.clone())
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(PnprClientError::Server(format!(
                    "PUT {} returned {}",
                    self.base_url,
                    response.status(),
                )));
            }
        }
        Ok(())
    }

    async fn exists(&self, url: &str) -> Result<bool, PnprClientError> {
        let status = self
            .authorize(self.http.head(url))
            .send()
            .await?
            .status();
        if status == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !status.is_success() {
            return Err(PnprClientError::Server(format!(
                "HEAD {} returned {status}",
                self.base_url,
            )));
        }
        Ok(true)
    }

    fn artifact_url(&self, owner: &OwnerScope, input_key: &str, scope: &str) -> String {
        format!(
            "{}/v8/artifacts/{}{}",
            self.base_url,
            artifact_hash(owner, input_key, scope),
            self.team_query,
        )
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.authorization {
            Some(authorization) => request.header("authorization", authorization),
            None => request,
        }
    }
}

/// The hash an artifact is stored under: a function of the owner, the input
/// key, and the compatibility scope, so a consumer can name it without
/// listing anything.
pub(crate) fn artifact_hash(owner: &OwnerScope, input_key: &str, scope: &str) -> String {
    pnpm_crypto_hash::create_hex_hash(&format!(
        "pnpm-shared-artifact:v1\0{}\0{input_key}\0{scope}",
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
