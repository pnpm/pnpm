use pnpm_network::{MetadataCacheScope, UpstreamRouteHook};
use pnpr_route::RouteHook;

/// A [`RouteHook`] bound to one package. The fetch helpers carry no
/// package, so without this a route rule that names one would not reach
/// the classification a resolve's fetch is made under.
///
/// The name bound in is the ecosystem's canonical spelling of it — a
/// crate's lowercase name, a Python project's PEP 503 name — which is what
/// each registry surface matches its own rules against. It names a package,
/// never a version of one.
pub(super) struct PackageRoute {
    hook: RouteHook,
    canonical_name: String,
}

impl PackageRoute {
    pub(super) fn new(hook: RouteHook, canonical_name: String) -> Self {
        Self { hook, canonical_name }
    }
}

impl UpstreamRouteHook for PackageRoute {
    fn authorize(&self, url: &str, _package: Option<&str>) -> Option<String> {
        self.hook.authorize(url, Some(&self.canonical_name))
    }

    fn allows_fetch(&self, url: &str) -> bool {
        self.hook.allows_fetch(url)
    }

    fn metadata_scope(&self, url: &str, _package: Option<&str>) -> MetadataCacheScope {
        self.hook.metadata_scope(url, Some(&self.canonical_name))
    }
}
