use reqwest::{
    StatusCode,
    header::{self, HeaderValue},
};

use super::{
    FetchOutcome, RegistryError, Result, UNPRIORITIZED, UPSTREAM_DISCOVERY_BODY_LIMIT, Upstream,
    read_limited_body,
};

impl Upstream {
    /// Post a JSON payload to an upstream npm audit endpoint
    /// (e.g. `/-/npm/v1/security/advisories/bulk`, `/-/npm/v1/security/audits`,
    /// or `/-/npm/v1/security/audits/quick`).
    pub async fn post_audit(
        &self,
        endpoint_path: &str,
        body: &[u8],
    ) -> Result<FetchOutcome<Vec<u8>>> {
        self.ensure_available()?;
        let url = format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            endpoint_path.trim_start_matches('/'),
        );
        let client =
            self.http.client.acquire_for_url_without_redirects_with_priority(&url, UNPRIORITIZED)
                .await;
        let mut headers = self.request_headers(&url);
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let request = client
            .post(&url)
            .timeout(self.http.timeout)
            .headers(headers)
            .body(body.to_vec());
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        let limited = read_limited_body(response, UPSTREAM_DISCOVERY_BODY_LIMIT)
            .await
            .map_err(|err| {
                self.breaker.record_failure();
                RegistryError::UpstreamResponse { url: url.clone(), reason: err.to_string() }
            })?;
        if limited.truncated {
            self.breaker.record_failure();
            return Err(RegistryError::UpstreamResponse {
                url,
                reason: format!(
                    "response body exceeds the {UPSTREAM_DISCOVERY_BODY_LIMIT}-byte limit",
                ),
            });
        }
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(limited.bytes))
    }
}
