use super::{
    Arc,
    RouteClass,
    ServerMetrics,
    TarballRouter,
};
use crate::server::TarballRoute;

impl TarballRouter {
    pub(in super::super::super) fn with_metrics(mut self, metrics: Arc<ServerMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Classify the route of a tarball URL the resolver emits, counting the
    /// decision. A decision counts even if its frame is later dropped, and a
    /// cache hit replays earlier decisions without counting them.
    pub(super) fn classify_tarball(&self, fetch: &str, package: &str) -> RouteClass {
        let class = self.context.classify(&self.identity, fetch, Some(package));
        if let Some(metrics) = &self.metrics {
            metrics.resolver.record_tarball_route(match class {
                RouteClass::Public => TarballRoute::Upstream,
                RouteClass::Hosted { .. } => TarballRoute::Hosted,
                RouteClass::Proxied { .. } => TarballRoute::Gateway,
            });
        }
        class
    }
}
