//! Hooks the resolver tests use to swap the routing snapshot.

use super::{
    RoutingConfig,
    RoutingSnapshot,
    build_snapshot,
};

impl super::super::super::AppState {
    /// The resolver engine of `routing`, for tests outside the server.
    pub(crate) fn test_resolver<'a>(
        &self,
        routing: &'a RoutingSnapshot,
    ) -> &'a crate::resolver::Resolver {
        crate::resolver::Resolver::get_or_init(routing, &self.inner.resolver_shared, None, self)
    }

    /// Serve a rebuild of the current graph, as a stored graph change does.
    pub(crate) fn test_replace_routing(&self) {
        let current = self.inner.current_routing();
        let routing = RoutingConfig::clone(&current);
        let next = build_snapshot(
            &self.inner.config,
            routing,
            Some(&current),
            &self.inner.upstream_guard_context,
        );
        self.inner.replace_routing(next);
    }

    /// Clear cached resolutions and lockfile verdicts, as a grant reload does.
    pub(crate) fn test_forget_resolver_results(&self) {
        if let Some(resolver) = self.inner.resolver_shared.get() {
            resolver.forget_results();
        }
    }

    /// Run `check` while holding the grant-reload transition guard.
    pub(crate) fn test_with_grant_transition(&self, check: impl FnOnce()) {
        let _transition = self.inner.identity.managed.grants.begin_transition();
        check();
    }
}
