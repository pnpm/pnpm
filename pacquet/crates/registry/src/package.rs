use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pacquet_network::{AuthHeaders, ThrottledClient};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};

use crate::{
    NetworkError, RegistryError, package_version::PackageVersion, package_versions::PackageVersions,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
    pub versions: PackageVersions,

    /// Per-version publish timestamps as the npm registry reports
    /// them. Each key is either a version string (value: ISO-8601
    /// timestamp) or the reserved `unpublished` key (value: object).
    /// The map is typed as `serde_json::Value` so the reserved key's
    /// object value can round-trip alongside the per-version
    /// timestamps without a custom deserializer.
    ///
    /// Mirrors pnpm's
    /// [`PackageMeta.time`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L11)
    /// and is the input to the `minimumReleaseAge` verifier. Use
    /// [`Self::published_at`] for the typed per-version lookup.
    ///
    /// Optional — abbreviated metadata responses (`application/vnd.npm.install-v1+json`)
    /// omit this field; only the full-metadata fetcher used by the
    /// verifier sees it populated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<HashMap<String, serde_json::Value>>,

    /// Package-level "last modified" timestamp the abbreviated
    /// metadata endpoint sends. The verifier's
    /// `tryAbbreviatedModifiedShortcut` reads this as a conservative
    /// upper bound on every version's publish time — if `modified`
    /// is older than the policy cutoff, every version in this
    /// package was published at least that long ago.
    ///
    /// Mirrors pnpm's
    /// [`PackageMeta.modified`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,

    /// Last `ETag` the registry returned when this manifest was
    /// fetched. Threaded into `If-None-Match` on the next
    /// conditional GET by the cached metadata fetcher (Phase 5).
    ///
    /// Mirrors pnpm's
    /// [`PackageMeta.etag`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Package-level `homepage` URL, shown in the `Details` column of
    /// `pacquet outdated --long`. Mirrors pnpm's
    /// [`PackageManifest.homepage`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/packages/types/src/package.ts).
    ///
    /// Only the full-metadata endpoint (`application/json`) carries this
    /// field; the abbreviated install metadata pacquet fetches by default
    /// (`application/vnd.npm.install-v1+json`) omits it, so it is `None`
    /// unless the registry serves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub mutex: Arc<Mutex<u8>>,
}

impl Package {
    /// Resolved publish timestamp for `version`, or `None` when the
    /// registry didn't report one for that pin. Filters out the
    /// reserved `unpublished` key (which is an object, not a string)
    /// and any version slot whose value isn't a string.
    #[must_use]
    pub fn published_at(&self, version: &str) -> Option<&str> {
        self.time.as_ref()?.get(version)?.as_str()
    }

    /// Version under `dist-tags.<tag>`, or `None` when the tag is
    /// absent. The picker reads `latest` (for the version-range fast
    /// path) and any user-supplied tag (e.g. `next`, `beta`) through
    /// this accessor.
    pub fn dist_tag(&self, tag: &str) -> Option<&str> {
        self.dist_tags.get(tag).map(String::as_str)
    }

    /// Iterator over all `dist-tags` entries. Used by the picker's
    /// publishedBy filter which rewrites tags after dropping versions
    /// past the cutoff. Iteration order is undefined (`HashMap`), as it
    /// is in upstream's JS where `Object.entries(distTags)` walks
    /// insertion order — neither stack guarantees a particular order
    /// to callers, so callers that need a stable rewrite are expected
    /// to sort.
    pub fn dist_tags(&self) -> impl Iterator<Item = (&str, &str)> {
        self.dist_tags.iter().map(|(tag, version)| (tag.as_str(), version.as_str()))
    }
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

    #[must_use]
    pub fn pinned_version(&self, version_range: &str) -> Option<Arc<PackageVersion>> {
        let range: node_semver::Range = version_range.parse().unwrap(); // TODO: this step should have happened in PackageManifest
        // Match on the version *strings* so only winning manifests
        // hydrate from their raw fragments. Walk the candidates from
        // highest to lowest: an undecodable fragment behaves as if
        // the version were absent, so the next-best match wins
        // instead of the lookup reporting no match at all.
        let mut satisfying = self
            .versions
            .keys()
            .filter_map(|key| {
                key.parse::<node_semver::Version>()
                    .ok()
                    .filter(|version| version.satisfies(&range))
                    .map(|version| (version, key))
            })
            .collect::<Vec<_>>();
        satisfying.sort_by(|(left, _), (right, _)| right.partial_cmp(left).unwrap());
        satisfying.into_iter().find_map(|(_, key)| self.versions.get(key))
    }

    /// Manifest under `dist-tags.latest`. `None` when the tag is
    /// absent, names a version the packument doesn't carry, or the
    /// version's fragment fails to decode — registry-served data must
    /// not be able to panic the process.
    #[must_use]
    pub fn latest(&self) -> Option<Arc<PackageVersion>> {
        self.versions.get(self.dist_tags.get("latest")?)
    }
}

#[cfg(test)]
mod tests;
