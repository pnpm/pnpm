use std::{
    fmt::Write,
    sync::atomic::{
        AtomicU64,
        Ordering,
    },
};

/// Who a cached resolution may be served to: a public footprint matches every
/// caller, a private one only callers that pass its access descriptors.
#[derive(Clone, Copy)]
pub(crate) enum ResolutionFootprint {
    Public,
    Private,
}

impl ResolutionFootprint {
    const ALL: [Self; 2] = [Self::Public, Self::Private];

    pub(crate) fn of(footprint: &pnpr_route::Footprint) -> Self {
        if footprint.is_public() { Self::Public } else { Self::Private }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

/// Where a resolver response points a client for a tarball.
#[derive(Clone, Copy)]
pub(crate) enum TarballRoute {
    /// The upstream URL of a public route.
    Upstream,
    /// pnpr's install gateway for a private proxied route.
    Gateway,
    /// pnpr's own tarball URL for a private hosted package.
    Hosted,
}

impl TarballRoute {
    const ALL: [Self; 3] = [Self::Upstream, Self::Gateway, Self::Hosted];

    const fn label(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Gateway => "gateway",
            Self::Hosted => "hosted",
        }
    }
}

/// Resolver counters for operators. Clients never see them.
#[derive(Default)]
pub(crate) struct ResolverMetrics {
    cache_hits: [AtomicU64; 2],
    cache_misses: AtomicU64,
    cache_stores: [AtomicU64; 2],
    tarball_routes: [AtomicU64; 3],
}

impl ResolverMetrics {
    pub(crate) fn record_cache_hit(&self, footprint: ResolutionFootprint) {
        self.cache_hits[footprint as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cache_store(&self, footprint: ResolutionFootprint) {
        self.cache_stores[footprint as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_tarball_route(&self, route: TarballRoute) {
        self.tarball_routes[route as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn render(&self, buffer: &mut String) {
        self.render_resolution_cache(buffer);
        let routes = TarballRoute::ALL.map(|route| {
            (route.label(), self.tarball_routes[route as usize].load(Ordering::Relaxed))
        });
        render_labelled_counter(
            buffer,
            &LabelledCounter {
                name: "pnpr_tarball_routes_total",
                help: "Tarball URLs the resolver routed, by destination.",
                label: "route",
                values: &routes,
            },
        );
    }

    fn render_resolution_cache(&self, buffer: &mut String) {
        let by_footprint = |counters: &[AtomicU64; 2]| {
            ResolutionFootprint::ALL.map(|footprint| {
                (footprint.label(), counters[footprint as usize].load(Ordering::Relaxed))
            })
        };
        render_labelled_counter(
            buffer,
            &LabelledCounter {
                name: "pnpr_resolution_cache_hits_total",
                help: "Resolution cache hits, by the footprint of the served entry.",
                label: "footprint",
                values: &by_footprint(&self.cache_hits),
            },
        );
        writeln!(
            buffer,
            "# HELP pnpr_resolution_cache_misses_total Resolution cache lookups that found no entry the caller may use.",
        )
        .unwrap();
        writeln!(buffer, "# TYPE pnpr_resolution_cache_misses_total counter").unwrap();
        let misses = self.cache_misses.load(Ordering::Relaxed);
        writeln!(buffer, "pnpr_resolution_cache_misses_total {misses}\n").unwrap();
        render_labelled_counter(
            buffer,
            &LabelledCounter {
                name: "pnpr_resolution_cache_stores_total",
                help: "Resolutions stored in the resolution cache, by footprint.",
                label: "footprint",
                values: &by_footprint(&self.cache_stores),
            },
        );
    }
}

struct LabelledCounter<'a> {
    name: &'a str,
    help: &'a str,
    label: &'a str,
    values: &'a [(&'a str, u64)],
}

fn render_labelled_counter(buffer: &mut String, counter: &LabelledCounter<'_>) {
    let LabelledCounter { name, help, label, values } = counter;
    writeln!(buffer, "# HELP {name} {help}").unwrap();
    writeln!(buffer, "# TYPE {name} counter").unwrap();
    for (value, count) in *values {
        writeln!(buffer, r#"{name}{{{label}="{value}"}} {count}"#).unwrap();
    }
    buffer.push('\n');
}
