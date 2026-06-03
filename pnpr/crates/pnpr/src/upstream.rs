use crate::{
    error::{RegistryError, Result},
    package_name::PackageName,
};
use pacquet_network::ThrottledClient;
use reqwest::StatusCode;
use serde_json::Value;
use std::sync::Arc;

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
/// (`application/vnd.npm.install-v1+json`) form. `time` goes beyond
/// the npm spec but the pnpm/pacquet resolvers read it for the
/// `minimumReleaseAge` check, so it stays.
///
/// `modified` isn't here because it's synthesized rather than copied:
/// it's extracted from `time.modified` (real npm packuments nest it
/// there). pacquet reads `meta.modified` in its version-pick
/// heuristics (`pick_package_from_meta.rs`) and as a freshness check
/// (`pick_package.rs`); omitting it pushes the resolver onto a slower
/// fallback path.
const ABBREVIATED_TOP_FIELDS: &[&str] = &["name", "dist-tags", "time"];

/// Per-version fields preserved in the abbreviated form — a subset of
/// the npm spec's abbreviated version object
/// (<https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object>).
/// Fields neither the pnpm nor the pacquet resolver reads are dropped
/// to shrink the document: `funding`, `acceptDependencies`,
/// `_hasShrinkwrap`, and `devDependencies` (a dependency's dev
/// dependencies are never installed). `dist.shasum` is dropped
/// per-version by [`drop_redundant_shasum`] when `dist.integrity` is
/// present.
const ABBREVIATED_VERSION_FIELDS: &[&str] = &[
    "name",
    "version",
    "deprecated",
    "bin",
    "dist",
    "engines",
    "directories",
    "dependencies",
    "peerDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "cpu",
    "os",
    "libc",
    "peerDependenciesMeta",
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
        // level.
        if let Some(time_modified) = obj.get("time").and_then(|time| time.get("modified")) {
            out.insert("modified".to_string(), time_modified.clone());
        }
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
                drop_redundant_shasum(&mut trimmed);
                abbreviated_versions.insert(version_id.clone(), Value::Object(trimmed));
            }
            out.insert("versions".to_string(), Value::Object(abbreviated_versions));
        }
    }
    Value::Object(out)
}

/// Drop the legacy `dist.shasum` (sha1) when `dist.integrity` (SRI) is
/// present. The pnpm and pacquet resolvers prefer `integrity` and only
/// fall back to `shasum` when `integrity` is absent (pre-2017
/// publishes), so shipping both is a redundant hash on every version.
fn drop_redundant_shasum(version: &mut serde_json::Map<String, Value>) {
    if let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut)
        && dist.get("integrity").is_some_and(|integrity| !integrity.is_null())
    {
        dist.remove("shasum");
    }
}

#[cfg(test)]
mod tests;
