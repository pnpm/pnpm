use super::{ResolveRequest, Resolver};
use pnpm_network::{AuthHeaders, UpstreamRouteHook};
use pnpr_policy::Identity;
use pnpr_route::{Footprint, RouteHook};
use std::sync::{Arc, Mutex};

impl Resolver {
    /// Build the request's [`AuthHeaders`] with the route hook installed:
    /// every metadata/tarball fetch is classified against this server's
    /// route policy for `identity`, the pnpr-managed credential (never the
    /// client's) is selected, and the touched private routes accumulate in
    /// `footprint`. The client's forwarded `auth_headers` are kept on the
    /// value (so `to_by_scope` still reflects them) but no longer consulted.
    pub(super) fn hooked_auth(
        &self,
        request: &ResolveRequest,
        identity: &Identity,
        footprint: &Arc<Mutex<Footprint>>,
    ) -> Arc<AuthHeaders> {
        let hook: Arc<dyn UpstreamRouteHook> = Arc::new(
            RouteHook::new(
                Arc::clone(&self.route_context),
                identity.clone(),
                Arc::clone(footprint),
                Arc::clone(&self.cache.secret),
            )
            .with_local_fetch(self.local_registry.fetcher(identity.clone())),
        );
        Arc::new(AuthHeaders::from_by_scope(request.auth_headers.clone()).with_route_hook(hook))
    }
}
