use super::{
    Arc, FetchAttestationOptions, FetchFullMetadataCachedOptions, NpmResolutionVerifier, OnceCell,
    Package, Pipe, PkgName, PublishedAtTimeMap, TrustHistoryProjection,
    fetch_attestation_published_at, fetch_full_metadata_cached, fetch_full_metadata_projected,
    load_local_meta_time, package_key, project_abbreviated_meta, project_trust_meta,
    redact_url_credentials, render_fetch_metadata_error,
};

impl NpmResolutionVerifier {
    /// Per-`(registry, name)` abbreviated-meta lookup. The result is
    /// projected down to `(modified, versionNames)` and cached so
    /// repeat verifications of the same package within an install
    /// cost at most one disk/network round-trip.
    ///
    /// Three fetch layers:
    /// 1. The shared [`crate::pick_package::PackageMetaCache`] populated by the resolver
    ///    during its own `pick_package` pass. Either form (full or
    ///    abbreviated) carries the two fields the projection needs,
    ///    so the verifier prefers `name:full` when present and falls
    ///    back to the bare `name` key.
    /// 2. The on-disk + network cached fetcher
    ///    ([`fetch_full_metadata_cached()`] with `full_metadata: false`)
    ///    when no shared entry is available.
    /// 3. A failure (decode / network / cache-write IO) caches a
    ///    credential-safe `Err(reason)` so subsequent calls reuse the
    ///    same verdict without retrying. The tarball-URL check surfaces
    ///    this error; the age shortcut ignores it and falls through to
    ///    the next layer of [`Self::resolve_published_at`].
    pub(super) async fn fetch_abbreviated_meta(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<crate::lookup_context::AbbreviatedMetaProjection, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.abbreviated_meta.lock().await;
            Arc::clone(
                cache
                    .entry(key)
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        let value = cell.get_or_init(|| async {
            // A shared packument that turns out to hold a damaged mirror
            // fragment falls through to the fetch below, which repairs it.
            if let Some(shared) = self.read_shared_meta(registry, name) {
                let projection =
                    project_abbreviated_meta(&shared, self.metadata.registry_supports_time_field);
                if !shared.versions.has_corrupt_mirror_fragment() {
                    return Ok(projection);
                }
            }
            let opts = FetchFullMetadataCachedOptions {
                registry,
                cache_dir: self.metadata.cache_dir.as_deref(),
                full_metadata: false,
                filter_metadata: false,
                offline: self.metadata.offline,
                priority: pnpm_network::BACKGROUND,
                http: crate::MetadataHttpClient {
                    http_client: &self.metadata.http_client,
                    auth_headers: &self.metadata.auth_headers,
                    retry_opts: self.metadata.retry_opts,
                },
            };
            // Carry a fetch failure (auth/network/5xx) as the `Err` value
            // instead of collapsing it to a missing projection: the
            // tarball-URL check needs to tell a transport failure apart
            // from a version genuinely absent from the metadata, otherwise
            // it reports a 403 as a tampering-style mismatch.
            fetch_full_metadata_projected(&name.to_string(), &opts, |meta| {
                project_abbreviated_meta(meta, self.metadata.registry_supports_time_field)
            })
            .await
            .map_err(|error| render_fetch_metadata_error(&error))
        })
        .await;
        value.clone()
    }

    /// Try the resolver's shared [`crate::pick_package::PackageMetaCache`] for a packument
    /// the abbreviated projection can derive from. The resolver keys
    /// entries by registry plus name (see `metadata_cache_key`), with a
    /// `:full` / `:full:filtered` suffix depending on its own metadata
    /// mode, so try full, filtered full, then abbreviated — a full form
    /// is a strict superset of the abbreviated shape, and `clear_meta`
    /// keeps every field the projection reads. Private-scoped entries
    /// carry a descriptor prefix the verifier can't reproduce; those
    /// simply miss and fall through to the verifier's own fetch chain.
    pub(super) fn read_shared_meta(&self, registry: &str, name: &PkgName) -> Option<Arc<Package>> {
        let cache = self.metadata.meta_cache.as_ref()?;
        let name_str = name.to_string();
        let key = package_key(registry, &name_str);
        cache
            .get(&format!("{key}:full"))
            .or_else(|| cache.get(&format!("{key}:full:filtered")))
            .or_else(|| cache.get(&key))
            .map(|cached| cached.meta)
            .filter(|meta| meta.name == name_str)
    }

    /// Per-`(registry, name)` on-disk mirror read of the full
    /// packument's per-version `time` map. Returns `None` when no
    /// mirror exists yet, no `cache_dir` was supplied, or the mirror
    /// has no `time` payload — the caller then falls through to the
    /// next layer of [`Self::resolve_published_at`].
    pub(super) async fn read_local_meta_time(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Option<Arc<PublishedAtTimeMap>> {
        let cache_dir = self.metadata.cache_dir.as_deref()?;
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.local_meta.lock().await;
            Arc::clone(
                cache
                    .entry(key)
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        // The verifier reads the *same* scoped mirror a resolve would
        // populate. A private packument lives under its descriptor
        // namespace, so a caller who can't reproduce the descriptor can't
        // read another caller's private `time` map through the trust-check
        // path.
        let name_string = name.to_string();
        let url = crate::registry_url::to_registry_url(registry, &name_string);
        let scope = self.metadata.auth_headers.metadata_scope(&url, Some(&name_string));
        cell.get_or_init(|| load_local_meta_time(cache_dir, &scope, registry, &name_string))
            .await
            .clone()
    }

    pub(super) async fn fetch_attestation_time(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if self.metadata.offline {
            return Ok(None);
        }
        let opts = FetchAttestationOptions {
            registry,
            http_client: &self.metadata.http_client,
            auth_headers: &self.metadata.auth_headers,
        };
        fetch_attestation_published_at(&name.to_string(), version, &opts).await
            .map_err(|err| redact_url_credentials(&err.to_string()))
    }

    pub(super) async fn fetch_full_meta_time(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Option<Arc<PublishedAtTimeMap>>, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.full_meta.lock().await;
            Arc::clone(
                cache
                    .entry(key)
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        cell.get_or_init(|| async {
            let pkg = self.metadata.fetch_full_meta(registry, name).await?;
            let time_map = pkg.time
                .as_ref()
                .map(|raw| {
                    raw.iter()
                        .filter_map(|(version, value)| {
                            let timestamp = value.as_str()?;
                            Some((version.clone(), timestamp.to_string()))
                        })
                        .collect::<PublishedAtTimeMap>()
                        .pipe(Arc::new)
                });
            Ok(time_map)
        })
        .await
        .clone()
    }

    pub(super) async fn fetch_full_meta_for_trust(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Arc<TrustHistoryProjection>, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.full_meta_for_trust.lock().await;
            Arc::clone(
                cache
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        cell.get_or_init(|| async {
            // Fast path: if the resolver already pulled the full packument
            // during the same install (`{registry}\x00{name}:full` or
            // `...:full:filtered` key in the shared metaCache, populated
            // when `pick_package` upgrades for `minimumReleaseAge`),
            // reuse it. The filtered form is accepted: `clear_meta`
            // keeps `time`, per-version `_npmUser`, and `dist`, which is
            // everything `fail_if_trust_downgraded` reads. Abbreviated
            // entries are rejected — they lack per-version `time` and
            // trust evidence.
            let shared = self.metadata.meta_cache
                .as_ref()
                .and_then(|cache| {
                    cache
                        .get(&format!("{key}:full"))
                        .or_else(|| cache.get(&format!("{key}:full:filtered")))
                });
            if let Some(cached) = shared {
                let projection = project_trust_meta(cached.meta.as_ref());
                if !cached.meta.versions.has_corrupt_mirror_fragment() {
                    return Ok(Arc::new(projection));
                }
            }
            self.metadata
                .fetch_full_meta_projected(registry, name, project_trust_meta)
                .await
                .map(Arc::new)
        })
        .await
        .clone()
    }
}

impl super::VerificationMetadataClient {
    pub(super) async fn fetch_full_meta(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Package, String> {
        fetch_full_metadata_cached(&name.to_string(), &self.full_meta_options(registry))
            .await
            .map_err(|error| render_fetch_metadata_error(&error))
    }

    pub(super) async fn fetch_full_meta_projected<Projection>(
        &self,
        registry: &str,
        name: &PkgName,
        project: impl Fn(&Package) -> Projection,
    ) -> Result<Projection, String> {
        let opts = self.full_meta_options(registry);
        fetch_full_metadata_projected(&name.to_string(), &opts, project).await
            .map_err(|error| render_fetch_metadata_error(&error))
    }

    fn full_meta_options<'a>(&'a self, registry: &'a str) -> FetchFullMetadataCachedOptions<'a> {
        FetchFullMetadataCachedOptions {
            registry,
            cache_dir: self.cache_dir.as_deref(),
            // The verifier reads `time` and trust evidence per-version,
            // both of which the abbreviated form drops. Always full.
            full_metadata: true,
            filter_metadata: false,
            offline: self.offline,
            priority: pnpm_network::BACKGROUND,
            http: crate::MetadataHttpClient {
                http_client: &self.http_client,
                auth_headers: &self.auth_headers,
                retry_opts: self.retry_opts,
            },
        }
    }
}
