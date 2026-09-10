use super::{
    Arc, AuthEntry, AuthHeaders, DEFAULT_REGISTRY_SCOPE, ParsedUrl, execute_token_helper,
    is_url_secure_for_credentials, package_scope, run_token_helper_command,
};

impl AuthHeaders {
    /// Resolve an `Authorization` header for `url`, preferring
    /// package-scope credentials when `pkg_name` is scoped.
    #[must_use]
    pub fn for_url_with_package(&self, url: &str, pkg_name: Option<&str>) -> Option<String> {
        if self.require_secure_transport && !is_url_secure_for_credentials(url) {
            return None;
        }
        // A server route hook owns the decision: ignore the
        // client-forwarded credentials entirely (including any inline
        // `user:pass@` in `url`) and let the deployment's policy pick the
        // credential and record the route.
        if let Some(hook) = &self.route_hook {
            return hook.authorize(url, pkg_name);
        }
        // Append a trailing `/` before parsing. Without this, a URL like
        // `https://npm.pkg.github.com/pnpm` (registry without
        // trailing slash) would nerf-dart to `//npm.pkg.github.com/`
        // and miss a `//npm.pkg.github.com/pnpm/` token.
        let mut owned: String;
        let url_with_slash = if url.ends_with('/') {
            url
        } else {
            owned = String::with_capacity(url.len() + 1);
            owned.push_str(url);
            owned.push('/');
            owned.as_str()
        };
        let parsed = ParsedUrl::parse(url_with_slash)?;
        if let Some(basic) = parsed.basic_auth_header() {
            return Some(basic);
        }
        if let Some(scope) = package_scope(pkg_name)
            && let Some(resolved) = self.lookup_with_port_fallback(&parsed, Some(scope))
        {
            return resolved;
        }
        self.lookup_with_port_fallback(&parsed, None)?
    }

    /// Look a URL's credential up, retrying without the port when the URL
    /// carries one — pnpm's `//host:port/` and `//host/` keys both apply.
    ///
    /// Returns `None` when no key matched (so the caller falls through to the
    /// next candidate) and `Some(_)` when a key matched — even `Some(None)`, a
    /// matched `tokenHelper` that failed to resolve. A match is final: pnpm's
    /// most-specific key owns the decision, so a failed helper must not fall
    /// back to a shorter prefix or a different scope and send another
    /// credential.
    pub(super) fn lookup_with_port_fallback(
        &self,
        parsed: &ParsedUrl<'_>,
        scope: Option<&str>,
    ) -> Option<Option<String>> {
        let lookup = |parsed: &ParsedUrl<'_>| match scope {
            Some(scope) => self.lookup_scope_by_nerf(parsed, scope),
            None => self.lookup_by_nerf(parsed),
        };
        if let Some(resolved) = lookup(parsed) {
            return Some(resolved);
        }
        parsed.port?;
        lookup(&parsed.with_port_stripped())
    }

    /// Walk package-scope keys for `scope` longest-prefix first. Returns
    /// `None` when nothing matched and `Some(resolved)` when a key
    /// matched (the inner `Option` is the resolved header, `None` if a
    /// matched `tokenHelper` failed).
    pub(super) fn lookup_scope_by_nerf(
        &self,
        parsed: &ParsedUrl<'_>,
        scope: &str,
    ) -> Option<Option<String>> {
        let scoped_by_uri = self.scoped_by_scope.get(scope)?;
        let max_scoped_parts = self.max_scoped_parts_by_scope.get(scope).copied()?;
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(max_scoped_parts);
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(entry) = scoped_by_uri.get(&key) {
                return Some(self.resolve_entry(&key, scope, entry));
            }
        }
        None
    }

    pub(super) fn lookup_by_nerf(&self, parsed: &ParsedUrl<'_>) -> Option<Option<String>> {
        if self.by_uri.is_empty() {
            return None;
        }
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(self.max_parts);
        // Walk from the longest meaningful prefix down to `//host/`.
        // `parts[0..3]` is `["", "", host]`, so joined with `/` it is
        // `//host`; the loop slices through `parts[..i]` and re-joins,
        // then appends a trailing slash. The exclusive upper bound at
        // `min(parts.len(), max_parts)` drops the extra iteration that
        // would always build a key ending in `//` (the trailing empty
        // segment from `nerfed.split('/')` plus the appended `/`) and
        // never match.
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(entry) = self.by_uri.get(&key) {
                return Some(self.resolve_entry(&key, DEFAULT_REGISTRY_SCOPE, entry));
            }
        }
        None
    }

    /// Resolve a matched [`AuthEntry`] to a header value. A baked header
    /// is cloned; a `tokenHelper` is executed once and memoized (keyed by
    /// `scope` + map key so the same command at two registries still runs
    /// per registry). A helper failure logs and yields `None` — pacquet
    /// never sends a wrong, partial, or stale credential.
    pub(super) fn resolve_entry(
        &self,
        key: &str,
        scope: &str,
        entry: &AuthEntry,
    ) -> Option<String> {
        match entry {
            AuthEntry::Header(value) => Some(value.clone()),
            AuthEntry::TokenHelper(command) => {
                let cache_key = format!("{scope}\u{0}{key}");
                // Take the per-key cell out under the global lock, then
                // release it *before* running the helper. Holding the lock
                // across the (up to `TOKEN_HELPER_TIMEOUT`) subprocess would
                // block every other registry's lookup on one slow helper.
                // `OnceLock` still serializes concurrent first-lookups of the
                // *same* key, so the command runs at most once.
                let cell = {
                    let mut cache = self
                        .resolved_token_helpers
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    Arc::clone(cache.entry(cache_key).or_default())
                };
                cell.get_or_init(|| {
                    let runner = self.token_helper_runner.unwrap_or(run_token_helper_command);
                    match execute_token_helper(command, runner) {
                        Ok(header) => Some(header),
                        Err(error) => {
                            let program = command.first().map_or("", String::as_str);
                            tracing::error!(
                                target: "pacquet::auth",
                                "token helper {program:?} failed; the request will be sent \
                                 without authentication: {error}",
                            );
                            None
                        }
                    }
                })
                .clone()
            }
        }
    }
}
