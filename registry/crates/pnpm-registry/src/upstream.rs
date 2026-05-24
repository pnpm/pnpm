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
/// glue specific to a proxy: building the upstream URL and fishing
/// the packument or tarball response out of it.
#[derive(Debug, Clone)]
pub struct Upstream {
    client: Arc<ThrottledClient>,
    base: String,
}

#[derive(Debug)]
pub enum FetchOutcome<Payload> {
    /// Upstream returned content.
    Ok(Payload),
    /// Upstream returned 404. The caller should propagate this verbatim.
    NotFound,
}

impl Upstream {
    pub fn new(base: String) -> Self {
        Self { client: Arc::new(ThrottledClient::new_for_installs()), base }
    }

    pub async fn fetch_packument(&self, name: &PackageName) -> Result<FetchOutcome<Vec<u8>>> {
        let url = format!("{}/{}", self.base.trim_end_matches('/'), name.as_str());
        self.fetch(&url).await
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

/// Rewrite every `versions[v].dist.tarball` in `value` to a URL
/// served by *this* registry instead of whatever URL the source put
/// there. The new URL is `{public_url}/{pkg}/-/{basename}`, where
/// `basename` is the last `/`-separated segment of the original
/// tarball URL. This handles both npm's canonical
/// `/{pkg}/-/{basename}` shape and verdaccio's
/// `/{scope}/{name}/-/{scope}/{filename}` shape uniformly — we only
/// look at the basename, never at the path prefix.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let public_url = public_url.trim_end_matches('/');
    let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for version in versions.values_mut() {
        let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(tarball_value) = dist.get_mut("tarball") else { continue };
        let Some(basename) = tarball_value.as_str().and_then(|url| url.rsplit('/').next()) else {
            continue;
        };
        let new_url = format!("{public_url}/{}/-/{basename}", pkg.as_str());
        *tarball_value = Value::String(new_url);
    }
}

#[cfg(test)]
mod tests {
    use super::rewrite_tarball_urls;
    use crate::package_name::PackageName;
    use serde_json::json;

    #[test]
    fn rewrites_npm_form_tarball() {
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
        let name = PackageName::parse("foo").unwrap();
        rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
        assert_eq!(
            doc["versions"]["1.0.0"]["dist"]["tarball"],
            "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
        );
        assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
    }

    #[test]
    fn rewrites_verdaccio_form_tarball_for_scoped() {
        // Verdaccio publishes scoped tarball URLs like
        // `/@scope/name/-/@scope/name-1.0.0.tgz` — the scope is
        // present twice. We only care about the basename.
        let mut doc = json!({
            "versions": {
                "1.0.0": {
                    "dist": {
                        "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz"
                    }
                }
            }
        });
        let name = PackageName::parse("@foo/no-deps").unwrap();
        rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
        assert_eq!(
            doc["versions"]["1.0.0"]["dist"]["tarball"],
            "http://127.0.0.1:9999/@foo/no-deps/-/no-deps-1.0.0.tgz",
        );
    }

    #[test]
    fn handles_packument_without_versions() {
        let mut doc = json!({ "name": "foo" });
        let name = PackageName::parse("foo").unwrap();
        rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
        assert_eq!(doc, json!({ "name": "foo" }));
    }
}
