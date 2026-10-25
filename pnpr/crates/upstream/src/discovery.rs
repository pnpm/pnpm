use reqwest::StatusCode;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use super::{
    FetchOutcome, RegistryError, Result, UNPRIORITIZED, UPSTREAM_DISCOVERY_BODY_LIMIT, Upstream,
    read_limited_body,
};

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub objects: Vec<Value>,
    pub total: usize,
}

impl Upstream {
    /// Query an upstream npm search endpoint with the caller's already-encoded
    /// query string. The upstream client contributes only configured headers,
    /// never headers supplied by the browser caller.
    pub async fn fetch_search(&self, query_string: &str) -> Result<FetchOutcome<SearchResponse>> {
        self.fetch_discovery_json(&format!("/-/v1/search?{query_string}")).await
    }

    /// Fetch the npm organization package map for one validated scope.
    pub async fn fetch_org_packages(
        &self,
        scope: &str,
    ) -> Result<FetchOutcome<Map<String, Value>>> {
        self.fetch_discovery_json(&format!("/-/org/{scope}/package")).await
    }

    async fn fetch_discovery_json<Payload: DeserializeOwned>(
        &self,
        path_and_query: &str,
    ) -> Result<FetchOutcome<Payload>> {
        self.ensure_available()?;
        let url = format!("{}{path_and_query}", self.base.trim_end_matches('/'));
        let client =
            self.http.client.acquire_for_url_without_redirects_with_priority(&url, UNPRIORITIZED)
                .await;
        let request = client
            .get(&url)
            .timeout(self.http.timeout)
            .headers(self.request_headers(&url));
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        let body = read_limited_body(response, UPSTREAM_DISCOVERY_BODY_LIMIT)
            .await
            .map_err(|err| {
                self.breaker.record_failure();
                RegistryError::UpstreamResponse { url: url.clone(), reason: err.to_string() }
            })?;
        if body.truncated {
            self.breaker.record_failure();
            return Err(RegistryError::UpstreamResponse {
                url,
                reason: format!(
                    "response body exceeds the {UPSTREAM_DISCOVERY_BODY_LIMIT}-byte limit",
                ),
            });
        }
        let parsed = serde_json::from_slice(&body.bytes)
            .map_err(|err| {
                self.breaker.record_failure();
                RegistryError::UpstreamResponse { url, reason: err.to_string() }
            })?;
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(parsed))
    }
}
