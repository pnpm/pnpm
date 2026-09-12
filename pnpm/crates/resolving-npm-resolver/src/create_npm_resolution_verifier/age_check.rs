use super::{
    Arc, DateTime, MINIMUM_RELEASE_AGE_VIOLATION_CODE, NpmResolutionVerifier, OnceCell, PkgName,
    ResolutionVerification, SkippedTimeCheck, Utc, package_key, parse_packument_timestamp,
    to_registry_url, uncheckable, version_key, warn_missing_time_once,
};

impl NpmResolutionVerifier {
    /// Tolerate an absent time map only when configured. An absent version in
    /// a complete map is an unpublished pin and must still fail closed.
    pub(super) async fn missing_publish_time_verdict(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Option<ResolutionVerification> {
        if self.ignore_missing_time_field
                // Already awaited by the lookup above, so this is a cache hit.
                && matches!(self.fetch_full_meta_time(registry, name).await, Ok(None))
        {
            warn_missing_time_once(&name.to_string(), SkippedTimeCheck::MinimumReleaseAge);
            return None;
        }
        Some(ResolutionVerification::Err {
            code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
            reason: uncheckable("minimumReleaseAge", "version not present in registry manifest"),
        })
    }

    pub(super) async fn run_age_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
        registry_name: Option<&str>,
    ) -> Option<ResolutionVerification> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        // Cheapest layer: for an entry whose canonical tarball this
        // install fetches (existence fail-closed by the fetch itself),
        // a package-level `Last-Modified` older than the cutoff bounds
        // every version's publish time — no metadata body needed. The
        // evidence cell is consulted before the probe so installs that
        // never fill it (no materialization, or a resolver alongside)
        // send no extra request.
        let planned_key =
            (name.to_string(), version.to_string(), registry_name.map(str::to_string));
        if self
            .planned_canonical_fetches
            .as_ref()
            .and_then(|cell| cell.get())
            .is_some_and(|planned| planned.contains(&planned_key))
            && self.head_modified_is_before(registry, name, cutoff).await
        {
            return None;
        }
        let published = match self.fetch_published_at(registry, name, version).await {
            Ok(value) => value,
            // A transport failure propagates the registry's own fetch error so
            // the install aborts with it; a successful fetch that merely lacks a
            // timestamp is handled below.
            Err(message) => return Some(ResolutionVerification::FetchFailed { message }),
        };
        let Some(published) = published else {
            return self.missing_publish_time_verdict(registry, name).await;
        };
        let Some(parsed) = parse_packument_timestamp(&published) else {
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: "publish timestamp is not a valid date".to_string(),
            });
        };
        if parsed > cutoff {
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: format!(
                    "was published at {published}, within the minimumReleaseAge cutoff ({cutoff})",
                    cutoff = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ),
            });
        }
        None
    }

    /// Whether the package-level `Last-Modified` a packument `HEAD`
    /// reports is older than `cutoff` by more than the header's own
    /// one-second resolution. HTTP dates carry whole seconds, and a
    /// registry that truncates a fractional `time.modified` understates
    /// it by up to 999ms — the guard band keeps the comparison an upper
    /// bound regardless of how the server rounds. `false` when the
    /// probe fails, the header is missing or unparsable, or the
    /// registry is unreachable — the caller falls through to the
    /// metadata-backed layers, so the probe can only ever *save* a
    /// body, never widen what passes. Trust-wise the header is the same
    /// statement as the packument body's `time.modified`, served by the
    /// same registry. One probe per `(registry, name)`, queued in the
    /// background network class.
    pub(super) async fn head_modified_is_before(
        &self,
        registry: &str,
        name: &PkgName,
        cutoff: DateTime<Utc>,
    ) -> bool {
        if self.offline {
            return false;
        }
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.head_modified.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        let modified = cell
            .get_or_init(|| async {
                let url = to_registry_url(registry, &name.to_string());
                let guard = self
                    .http_client
                    .acquire_for_url_with_priority(&url, pnpm_network::BACKGROUND)
                    .await;
                let mut request = guard.head(&url);
                if let Some(value) =
                    self.auth_headers.for_url_with_package(&url, Some(&name.to_string()))
                {
                    request = request.header("authorization", value);
                }
                let response = match request.send().await {
                    Ok(response) if response.status().is_success() => response,
                    _ => return None,
                };
                response
                    .headers()
                    .get("last-modified")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string)
            })
            .await;
        modified
            .as_deref()
            .and_then(|value| httpdate::parse_http_date(value).ok())
            .map(DateTime::<Utc>::from)
            .is_some_and(|parsed| parsed + chrono::Duration::seconds(1) <= cutoff)
    }

    /// Per-`(registry, name, version)` lookup with a layered fallback.
    pub(super) async fn fetch_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        let key = version_key(registry, &name.to_string(), version);
        let cell = {
            let mut cache = self.lookup_context.published_at.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        cell.get_or_init(|| async { self.resolve_published_at(registry, name, version).await })
            .await
            .clone()
    }

    /// Layered publish-timestamp lookup:
    ///
    /// 1. **Abbreviated-`modified` shortcut.** Abbreviated metadata is
    ///    a small per-name document the resolver typically already
    ///    holds. Its package-level `modified` is an upper bound on
    ///    every version's publish time — if it's older than the
    ///    cutoff *and* the pinned version is still listed in
    ///    `versions`, the gate is satisfied without per-version
    ///    timestamps. Costs at most one abbreviated GET per name on
    ///    cold cache; the full-meta fallback below is hundreds of KB
    ///    bigger per package.
    /// 2. **Abbreviated per-version `time`** — only with
    ///    `registrySupportsTimeField`. Registries that serve the `time`
    ///    map in abbreviated metadata (Verdaccio 5.15.1+, pnpr) already
    ///    gave us the exact per-version timestamp in the document step 1
    ///    fetched, so a recent `modified` does not have to escalate to
    ///    the per-version fallbacks below.
    /// 3. **On-disk full-meta mirror.** If a previous verification
    ///    populated `<cache_dir>/v11/metadata-full/.../<name>.jsonl`,
    ///    take the per-version timestamp from there with no network.
    /// 4. **Npm attestation endpoint.** Small payload, just this
    ///    version's Sigstore-anchored timestamp. Wins on cold cache
    ///    when the package was published with provenance.
    /// 5. **Full metadata fetch.** Last resort.
    pub(super) async fn resolve_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if let Some(value) = self.try_abbreviated_modified_shortcut(registry, name, version).await {
            return Ok(Some(value));
        }
        if self.registry_supports_time_field
            && let Some(value) = self.abbreviated_version_time(registry, name, version).await
        {
            return Ok(Some(value));
        }
        if let Some(map) = self.read_local_meta_time(registry, name).await
            && let Some(value) = map.get(version)
        {
            return Ok(Some(value.clone()));
        }
        if let Some(value) = self.fetch_attestation_time(registry, name, version).await? {
            return Ok(Some(value));
        }
        let full_meta_time = self.fetch_full_meta_time(registry, name).await?;
        Ok(full_meta_time.and_then(|map| map.get(version).cloned()))
    }

    /// Returns the package's `modified` timestamp *iff* it proves the
    /// gate would pass — i.e. it's strictly older than the policy
    /// cutoff *and* the pinned version is still listed in the
    /// package's current versions map.
    ///
    /// The version check is the fail-closed contract: an unpublished
    /// or never-published pin must not slip through on a stale
    /// package-level `modified` timestamp.
    pub(super) async fn try_abbreviated_modified_shortcut(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<String> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        // A fetch failure here is fine: ignore the error and fall back to
        // per-version lookups, the same as a successful-but-uninformative
        // metadata response.
        let Ok(meta) = self.fetch_abbreviated_meta(registry, name).await else {
            return None;
        };
        let modified = meta.modified?;
        let parsed = parse_packument_timestamp(&modified)?;
        if parsed >= cutoff {
            return None;
        }
        if !meta.version_artifacts.as_ref().is_some_and(|map| map.contains_key(version)) {
            return None;
        }
        Some(modified)
    }

    /// The pinned version's publish timestamp from the abbreviated
    /// document's `time` map. Reuses the same cached projection as the
    /// `modified` shortcut, so with `registrySupportsTimeField` the
    /// whole lookup stays within the one document the verifier already
    /// holds. A fetch failure or an absent entry falls through to the
    /// per-version fallbacks, exactly like the shortcut above.
    pub(super) async fn abbreviated_version_time(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<String> {
        let meta = self.fetch_abbreviated_meta(registry, name).await.ok()?;
        meta.version_time.as_ref()?.get(version).cloned()
    }
}
