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

/// Rewrite every `dist.tarball` in `value` to a URL served by *this*
/// registry instead of whatever URL the source put there. The new
/// URL is `{public_url}/{pkg}/-/{basename}`, where `basename` is the
/// last `/`-separated segment of the original tarball URL. This
/// handles both npm's canonical `/{pkg}/-/{basename}` shape and
/// verdaccio's `/{scope}/{name}/-/{scope}/{filename}` shape uniformly
/// — we only look at the basename, never at the path prefix.
///
/// Walks both packument shape (`{ "versions": { v: { dist: ... } } }`)
/// and single-version manifest shape (`{ dist: ... }` at the top
/// level) so a single helper covers both endpoints.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let public_url = public_url.trim_end_matches('/');
    if let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) {
        for version in versions.values_mut() {
            rewrite_dist_tarball(version, pkg, public_url);
        }
    }
    rewrite_dist_tarball(value, pkg, public_url);
}

fn rewrite_dist_tarball(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let Some(dist) = value.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(tarball_value) = dist.get_mut("tarball") else { return };
    let Some(basename) = tarball_value.as_str().and_then(|url| url.rsplit('/').next()) else {
        return;
    };
    *tarball_value = Value::String(format!("{public_url}/{}/-/{basename}", pkg.as_str()));
}

/// Look up the version manifest for `version_or_tag` inside a parsed
/// packument: if the string matches a dist-tag it resolves through
/// `dist-tags[tag]` first, otherwise it's taken as a literal version.
/// Returns the version's manifest *with* the `dist.tarball` rewritten
/// to point at this server.
pub fn extract_version_manifest(
    packument: &Value,
    pkg: &PackageName,
    version_or_tag: &str,
    public_url: &str,
) -> Option<Value> {
    let resolved = packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag);
    let mut manifest = packument.get("versions")?.get(resolved)?.clone();
    rewrite_tarball_urls(&mut manifest, pkg, public_url);
    Some(manifest)
}

/// Top-level packument fields *copied verbatim* into the abbreviated
/// (`application/vnd.npm.install-v1+json`) form. Mirrors verdaccio's
/// `convertAbbreviatedManifest` (packages/store/src/storage.ts).
/// `time` and `readme` go beyond the npm spec but verdaccio keeps
/// them — `time` specifically for pnpm's `minimumReleaseAge` check.
///
/// Two fields aren't here because verdaccio synthesizes them rather
/// than copying:
///   * `modified`        — extracted from `time.modified` (real npm
///     packuments don't have it at the top level)
///   * `readmeFilename`  — always the empty string
///
/// pacquet reads `meta.modified` in its version-pick heuristics
/// (`pick_package_from_meta.rs`) and as a freshness check
/// (`pick_package.rs`); omitting it pushes the resolver onto a
/// slower fallback path.
const ABBREVIATED_TOP_FIELDS: &[&str] = &["name", "dist-tags", "time", "_id", "_rev", "readme"];

/// Per-version fields preserved in the abbreviated form. Mirrors
/// verdaccio's `convertAbbreviatedManifest` and the npm spec at
/// <https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object>.
const ABBREVIATED_VERSION_FIELDS: &[&str] = &[
    "name",
    "version",
    "deprecated",
    "bin",
    "dist",
    "engines",
    "funding",
    "directories",
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "cpu",
    "os",
    "peerDependenciesMeta",
    "acceptDependencies",
    "_hasShrinkwrap",
    "hasInstallScript",
];

/// Strip a parsed packument down to the abbreviated install-v1 form.
/// Should be called *after* `rewrite_tarball_urls` so the returned
/// document's `dist.tarball` URLs already point at this server.
pub fn abbreviate_packument(packument: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = packument.as_object() {
        for &field in ABBREVIATED_TOP_FIELDS {
            if let Some(value) = obj.get(field) {
                out.insert(field.to_string(), value.clone());
            }
        }
        // Synthesize `modified` from `time.modified` — npm packuments
        // store it nested and pacquet's resolver reads it at the top
        // level. Matches verdaccio's `modified: manifest.time.modified`.
        if let Some(time_modified) = obj.get("time").and_then(|time| time.get("modified")) {
            out.insert("modified".to_string(), time_modified.clone());
        }
        // verdaccio hardcodes `readmeFilename: ''`. Match it for
        // wire-shape parity with clients that key on its presence.
        out.insert("readmeFilename".to_string(), Value::String(String::new()));
        if let Some(versions) = obj.get("versions").and_then(Value::as_object) {
            let mut abbreviated_versions = serde_json::Map::with_capacity(versions.len());
            for (version_id, version_value) in versions {
                let Some(version_obj) = version_value.as_object() else { continue };
                let mut trimmed = serde_json::Map::new();
                for &field in ABBREVIATED_VERSION_FIELDS {
                    if let Some(value) = version_obj.get(field) {
                        trimmed.insert(field.to_string(), value.clone());
                    }
                }
                abbreviated_versions.insert(version_id.clone(), Value::Object(trimmed));
            }
            out.insert("versions".to_string(), Value::Object(abbreviated_versions));
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::{extract_version_manifest, rewrite_tarball_urls};
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

    #[test]
    fn extracts_version_by_dist_tag() {
        let doc = json!({
            "name": "@foo/no-deps",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@foo/no-deps",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz",
                        "shasum": "abc"
                    }
                }
            }
        });
        let name = PackageName::parse("@foo/no-deps").unwrap();
        let manifest = extract_version_manifest(&doc, &name, "latest", "http://reg").unwrap();
        assert_eq!(manifest["version"], "1.0.0");
        assert_eq!(manifest["dist"]["tarball"], "http://reg/@foo/no-deps/-/no-deps-1.0.0.tgz");
        assert_eq!(manifest["dist"]["shasum"], "abc");
    }

    #[test]
    fn extracts_version_by_literal_version() {
        let doc = json!({
            "name": "foo",
            "versions": { "2.0.0": { "version": "2.0.0", "dist": { "tarball": "x/foo-2.0.0.tgz" } } }
        });
        let name = PackageName::parse("foo").unwrap();
        let manifest = extract_version_manifest(&doc, &name, "2.0.0", "http://reg").unwrap();
        assert_eq!(manifest["version"], "2.0.0");
        assert_eq!(manifest["dist"]["tarball"], "http://reg/foo/-/foo-2.0.0.tgz");
    }

    #[test]
    fn extract_returns_none_for_unknown_version() {
        let doc = json!({
            "versions": { "1.0.0": { "dist": { "tarball": "x/foo-1.0.0.tgz" } } }
        });
        let name = PackageName::parse("foo").unwrap();
        assert!(extract_version_manifest(&doc, &name, "9.9.9", "http://reg").is_none());
        assert!(extract_version_manifest(&doc, &name, "latest", "http://reg").is_none());
    }
}
