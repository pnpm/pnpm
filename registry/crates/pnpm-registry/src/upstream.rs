use std::sync::Arc;

use pacquet_network::ThrottledClient;
use reqwest::StatusCode;
use serde_json::Value;

use crate::error::{RegistryError, Result};
use crate::package_name::PackageName;

/// Wraps a shared [`ThrottledClient`] (so the registry inherits pnpm's
/// tuned reqwest defaults: `User-Agent: pnpm`, HTTP/1.1, hickory DNS,
/// pool/timeout tuning, concurrency semaphore, and per-registry TLS
/// routing if it's ever wired in later) and adds the small bit of
/// glue specific to a proxy: building the upstream URL, fishing the
/// packument out of the response, and rewriting `dist.tarball`.
#[derive(Debug, Clone)]
pub struct Upstream {
    client: Arc<ThrottledClient>,
    base: String,
    public_url: String,
}

#[derive(Debug)]
pub enum FetchOutcome<Payload> {
    /// Upstream returned content.
    Ok(Payload),
    /// Upstream returned 404. The caller should propagate this verbatim.
    NotFound,
}

impl Upstream {
    pub fn new(base: String, public_url: String) -> Self {
        Self { client: Arc::new(ThrottledClient::new_for_installs()), base, public_url }
    }

    /// Fetch a packument from the upstream and rewrite every
    /// `versions[v].dist.tarball` URL so it points back at this server.
    /// The rewritten JSON is what gets cached and served.
    pub async fn fetch_packument(&self, name: &PackageName) -> Result<FetchOutcome<Vec<u8>>> {
        let url = format!("{}/{}", self.base.trim_end_matches('/'), name.as_str());
        let bytes = match self.fetch(&url).await? {
            FetchOutcome::Ok(bytes) => bytes,
            FetchOutcome::NotFound => return Ok(FetchOutcome::NotFound),
        };
        let mut json: Value = serde_json::from_slice(&bytes)?;
        rewrite_tarball_urls(&mut json, &self.base, &self.public_url);
        Ok(FetchOutcome::Ok(serde_json::to_vec(&json)?))
    }

    /// Send the tarball request and return the streaming
    /// [`reqwest::Response`] so the caller can pipe the body straight
    /// to the client without buffering. Status and 404 handling
    /// happen here before any bytes are forwarded.
    pub async fn fetch_tarball_response(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<FetchOutcome<reqwest::Response>> {
        let url = format!("{}/{}/-/{}", self.base.trim_end_matches('/'), name.as_str(), filename);
        let client = self.client.acquire_for_url(&url).await;
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.clone(), source })?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(FetchOutcome::NotFound);
        }
        let response = check_status(response, &url).await?;
        Ok(FetchOutcome::Ok(response))
    }

    async fn fetch(&self, url: &str) -> Result<FetchOutcome<Vec<u8>>> {
        let client = self.client.acquire_for_url(url).await;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.to_string(), source })?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(FetchOutcome::NotFound);
        }
        let response = check_status(response, url).await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.to_string(), source })?;
        Ok(FetchOutcome::Ok(bytes.to_vec()))
    }
}

async fn check_status(response: reqwest::Response, url: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(RegistryError::UpstreamStatus { url: url.to_string(), status: status.as_u16(), body })
}

/// Walk a packument document and replace every string that starts with
/// `upstream` and refers to a `dist.tarball` URL with a corresponding
/// `public_url` URL.
fn rewrite_tarball_urls(value: &mut Value, upstream: &str, public_url: &str) {
    let upstream = upstream.trim_end_matches('/');
    let public_url = public_url.trim_end_matches('/');
    let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for version in versions.values_mut() {
        let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(tarball_value) = dist.get_mut("tarball") else { continue };
        let Some(tarball) = tarball_value.as_str() else { continue };
        if let Some(suffix) = tarball.strip_prefix(upstream) {
            *tarball_value = Value::String(format!("{public_url}{suffix}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rewrite_tarball_urls;
    use serde_json::json;

    #[test]
    fn rewrites_dist_tarball() {
        let mut doc = json!({
            "name": "foo",
            "versions": {
                "1.0.0": {
                    "dist": {
                        "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
                        "shasum": "abc"
                    }
                }
            }
        });
        rewrite_tarball_urls(&mut doc, "https://registry.npmjs.org", "http://127.0.0.1:4873");
        assert_eq!(
            doc["versions"]["1.0.0"]["dist"]["tarball"],
            "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
        );
        assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
    }

    #[test]
    fn leaves_other_hosts_alone() {
        let mut doc = json!({
            "versions": {
                "1.0.0": {
                    "dist": {
                        "tarball": "https://other.example.com/foo/-/foo-1.0.0.tgz"
                    }
                }
            }
        });
        rewrite_tarball_urls(&mut doc, "https://registry.npmjs.org", "http://127.0.0.1:4873");
        assert_eq!(
            doc["versions"]["1.0.0"]["dist"]["tarball"],
            "https://other.example.com/foo/-/foo-1.0.0.tgz",
        );
    }

    #[test]
    fn handles_packument_without_versions() {
        let mut doc = json!({ "name": "foo" });
        rewrite_tarball_urls(&mut doc, "https://registry.npmjs.org", "http://127.0.0.1:4873");
        assert_eq!(doc, json!({ "name": "foo" }));
    }
}
