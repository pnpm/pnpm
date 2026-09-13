use super::{JSR_ORIGIN, NPMJS_ORIGIN, PublicRoute, RouteMatcher, compile_glob, nerf_prefix};
impl RouteMatcher {
    /// The built-in public route: the official npm registry, host-level (no
    /// package glob, so scoped and unscoped packages alike are public). An
    /// anonymous fetch returns only public content — a private scoped package
    /// `404`s — so a successfully-resolved npmjs route is public and globally
    /// shareable. Prepended to the operator-declared routes in
    /// [`super::RouteContext::from_config`], so it is allowlisted and public without
    /// any config, and ahead of any upstream credential for the same origin.
    pub(super) fn npmjs() -> Self {
        Self {
            origin: Some(NPMJS_ORIGIN.to_string()),
            package: None,
            https_only: true,
        }
    }

    /// The other built-in public route: JSR, which every pnpm client routes
    /// the `@jsr` scope to without configuring anything, so a graph holding a
    /// JSR dependency resolves through a default server. It hosts no private
    /// content, so the reasoning in [`Self::npmjs`] applies unchanged.
    pub(super) fn jsr() -> Self {
        Self {
            origin: Some(JSR_ORIGIN.to_string()),
            package: None,
            https_only: true,
        }
    }

    /// Build a matcher from an operator-declared public route, failing
    /// closed. An *omitted* `registry`/`package` field means "match any" —
    /// the intended wildcard — but a field that is *present yet unparsable*
    /// (a typo'd registry URL or glob) drops the whole rule (`None`) rather
    /// than collapsing to a `None` field that [`Self::matches`] would read as
    /// match-any. A typo must narrow matching, never widen a scoped public
    /// route into a match-all that leaks private metadata onto the public
    /// path.
    pub(super) fn from_public_route(route: &PublicRoute) -> Option<Self> {
        let origin = match route.registry.as_deref() {
            None => None,
            Some(registry) => Some(
                nerf_prefix(registry)
                    .or_else(|| {
                        tracing::warn!(
                            registry,
                            "ignoring public route with an unparsable registry URL",
                        );
                        None
                    })?,
            ),
        };
        let package = match route.package.as_deref() {
            None => None,
            Some(pattern) => Some(
                compile_glob(pattern)
                    .or_else(|| {
                        tracing::warn!(
                            pattern,
                            "ignoring public route with an invalid package glob",
                        );
                        None
                    })?,
            ),
        };
        Some(Self {
            origin,
            package,
            https_only: false,
        })
    }

    /// Whether this route puts `fetch` (nerf-darted, so scheme-less) on the
    /// allowlist when reached over `scheme`. A package-scoped route with no
    /// registry allowlists nothing: it narrows an origin another rule already
    /// admits.
    pub(super) fn allowlists(&self, fetch: &str, scheme: Option<&str>) -> bool {
        self.origin
            .as_deref()
            .is_some_and(|origin| fetch.starts_with(origin))
            && (!self.https_only
                || scheme.is_some_and(|scheme| scheme.eq_ignore_ascii_case("https")))
    }

    pub(super) fn matches(&self, fetch: &str, package: Option<&str>) -> bool {
        let origin_ok = self.origin
            .as_deref()
            .is_none_or(|origin| fetch.starts_with(origin));
        let package_ok = self.package
            .as_ref()
            .is_none_or(|glob| package.is_some_and(|name| glob.is_match(name)));
        origin_ok && package_ok
    }
}

use wax::Program;
