use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pacquet_network::{AuthHeaders, ThrottledClient};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};

use crate::{NetworkError, RegistryError, package_version::PackageVersion};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Package {
    pub name: String,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    pub versions: HashMap<String, PackageVersion>,

    #[serde(skip_serializing, skip_deserializing)]
    pub mutex: Arc<Mutex<u8>>,
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Package {
    pub async fn fetch_from_registry(
        name: &str,
        http_client: &ThrottledClient,
        registry: &str,
        auth_headers: &AuthHeaders,
    ) -> Result<Self, RegistryError> {
        // Format once. The same string is consumed by the GET, the
        // per-URL `Authorization` lookup, and the error mapper — using
        // distinct closures risked the auth lookup and request URL
        // drifting if the format expression ever changed.
        let url = format!("{registry}{name}"); // TODO: use reqwest URL directly
        let network_error = |error| NetworkError { error, url: url.clone() };
        let mut request = http_client.acquire_for_url(&url).await.get(&url).header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        );
        // Mirrors `fetchMetadataFromFromRegistry` in pnpm v11's
        // [`resolving/npm-resolver/src/fetch.ts`](https://github.com/pnpm/pnpm/blob/601317e7a3/resolving/npm-resolver/src/fetch.ts):
        // resolve the per-URL `Authorization` value before issuing the
        // request and attach it when present.
        if let Some(value) = auth_headers.for_url(&url) {
            request = request.header("authorization", value);
        }
        request
            .send()
            .await
            .map_err(network_error)?
            .json::<Package>()
            .await
            .map_err(network_error)?
            .pipe(Ok)
    }

    pub fn pinned_version(&self, version_range: &str) -> Option<&PackageVersion> {
        let range: node_semver::Range = version_range.parse().unwrap(); // TODO: this step should have happened in PackageManifest
        let mut satisfied_versions = self
            .versions
            .values()
            .filter(|version| version.version.satisfies(&range))
            .collect::<Vec<&PackageVersion>>();

        satisfied_versions.sort_by(|a, b| a.version.partial_cmp(&b.version).unwrap());

        // Optimization opportunity:
        // We can store this in a cache to remove filter operation and make this a O(1) operation.
        satisfied_versions.last().copied()
    }

    pub fn latest(&self) -> &PackageVersion {
        let version =
            self.dist_tags.get("latest").expect("latest tag is expected but not found for package");
        self.versions.get(version).unwrap()
    }
}

#[cfg(test)]
mod tests;
