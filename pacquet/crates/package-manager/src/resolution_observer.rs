//! Resolver wrapper that reports each resolved tarball package as the
//! tree walk yields it.
//!
//! Unlike [`crate::PrefetchingResolver`], which kicks off a background
//! *download* per resolution, [`ObservingResolver`] does no I/O — it
//! forwards a lightweight fetch hint to a [`ResolutionObserver`]. The
//! pnpr server installs an observer that streams each hint to the client
//! as an NDJSON `package` frame, so the client begins fetching tarballs
//! while the server is still resolving
//! ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)).

use crate::install_package_from_registry::{
    extract_tarball, manifest_file_count, manifest_unpacked_size,
};
use dashmap::DashSet;
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use std::sync::Arc;

/// One resolved tarball-shaped package, surfaced as the resolver's tree
/// walk claims it. Carries exactly what a client needs to start fetching
/// the tarball before the full lockfile is assembled. Borrowed: the
/// observer copies whatever it needs out of the call.
pub struct ResolvedPackageHint<'a> {
    /// Canonical `name@version` identifier — the store-index
    /// `package_id` the install pass keys downloads by.
    pub id: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    /// Subresource-integrity string (`sha512-...`).
    pub integrity: &'a str,
    /// The resolver's `dist.tarball` URL — the same string the install
    /// pass looks up in the shared mem cache.
    pub tarball_url: &'a str,
    /// `dist.unpackedSize` from the resolver-fetched manifest, when
    /// the registry published one. Lets the fetching side size its
    /// decompression buffer exactly and start the largest archives
    /// first when the connection pool is saturated.
    pub unpacked_size: Option<usize>,
    /// `dist.fileCount` from the resolver-fetched manifest, when the
    /// registry published one. The per-file term of the download
    /// priority's pipeline-work estimate.
    pub file_count: Option<usize>,
}

/// Sink notified once per resolved tarball package during a resolve.
/// Implemented by the pnpr server to stream fetch frames to the client.
pub trait ResolutionObserver: Send + Sync {
    fn on_resolved(&self, hint: ResolvedPackageHint<'_>);
}

/// Wraps an inner [`Resolver`], forwarding each tarball-shaped result to
/// a [`ResolutionObserver`] as the deps-resolver claims it.
///
/// Non-tarball resolutions (git, directory, registry-shape, binary,
/// variations) and results missing a structured `name@version` are
/// skipped — the client fetches those through their own protocol paths,
/// not a plain tarball download.
pub struct ObservingResolver {
    inner: Box<dyn Resolver>,
    observer: Arc<dyn ResolutionObserver>,
    /// Tarball URLs already reported. The deps-resolver calls `resolve`
    /// once per `(parent, child)` edge, so the same package surfaces many
    /// times; dedup by URL collapses those to a single frame. Mirrors
    /// `PrefetchingResolver::spawned_urls`.
    seen: DashSet<String>,
}

impl ObservingResolver {
    pub fn new(inner: Box<dyn Resolver>, observer: Arc<dyn ResolutionObserver>) -> Self {
        ObservingResolver { inner, observer, seen: DashSet::new() }
    }

    fn maybe_report(&self, result: &ResolveResult) {
        // Gate on the same tarball shape `extract_tarball` accepts —
        // other resolution shapes aren't fetched as plain tarballs.
        let Ok((tarball_url, integrity)) = extract_tarball(&result.resolution) else {
            return;
        };
        let Some(name_ver) = result.name_ver.as_ref() else {
            return;
        };
        if !self.seen.insert(tarball_url.to_string()) {
            return;
        }
        let id = name_ver.to_string();
        let name = name_ver.name.to_string();
        let version = name_ver.suffix.to_string();
        let integrity = integrity.to_string();
        self.observer.on_resolved(ResolvedPackageHint {
            id: &id,
            name: &name,
            version: &version,
            integrity: &integrity,
            tarball_url,
            unpacked_size: manifest_unpacked_size(result.manifest.as_deref()),
            file_count: manifest_file_count(result.manifest.as_deref()),
        });
    }
}

impl Resolver for ObservingResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let result = self.inner.resolve(wanted_dependency, opts).await?;
            if let Some(result_ref) = result.as_ref() {
                self.maybe_report(result_ref);
            }
            Ok(result)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        self.inner.resolve_latest(query, opts)
    }
}
