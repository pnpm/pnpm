use std::collections::HashSet;

use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey, PackageMetadata, TarballRevision};
use pnpm_package_manager::{
    ResolvedPackageHint, ResolvedPackageIdentity, tarball_url_and_integrity,
};
use pnpm_resolving_npm_resolver::{DistStats, ObservedDistStats};

use super::{MAX_NDJSON_FRAME_BYTES, TarballRouter, ndjson_line, package_frame};

/// `package` frames for every tarball-fetchable entry of a verified
/// frozen lockfile, deduplicated by tarball URL. Mirrors what the
/// streaming resolve's [`StreamObserver`](super::StreamObserver) would have
/// announced had the tree walk run: the client prefetches each tarball on
/// arrival, with `unpackedSize` (from the verification fan-out's metadata, when
/// the registry published one) prioritizing the largest downloads.
///
/// Tarball URLs are derived with the same
/// [`tarball_url_and_integrity`] the client's frozen materialization
/// uses, so the announced URLs match the client's mem-cache keys
/// byte-for-byte. Non-tarball resolutions (git, directory, binary,
/// variations) are skipped — the client fetches those through their
/// own protocol paths.
pub(in super::super) fn frozen_package_frames_until(
    config: &PacquetConfig,
    router: &TarballRouter,
    lockfile: &Lockfile,
    dist_stats: &ObservedDistStats,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Vec<Vec<u8>>> {
    if should_cancel() {
        return None;
    }
    let Some(packages) = lockfile.packages.as_ref() else {
        return Some(Vec::new());
    };
    let mut seen_urls = HashSet::new();
    let mut frames = Vec::new();
    let mut total_bytes = 0usize;
    for (package_key, snapshot) in packages {
        if should_cancel() {
            return None;
        }
        if let Some(line) = frozen_package_frame(
            FrozenFrameInputs { config, router, dist_stats, package_key, snapshot },
            &mut seen_urls,
        ) {
            if total_bytes.saturating_add(line.len()) > MAX_NDJSON_FRAME_BYTES {
                break;
            }
            total_bytes += line.len();
            frames.push(line);
        }
    }
    Some(frames)
}

#[derive(Clone, Copy)]
struct FrozenFrameInputs<'a> {
    config: &'a PacquetConfig,
    router: &'a TarballRouter,
    dist_stats: &'a ObservedDistStats,
    package_key: &'a PackageKey,
    snapshot: &'a PackageMetadata,
}

/// The `package` frame for one frozen lockfile entry, or `None` when the
/// entry is not a registry or tarball package or its URL was announced
/// already.
fn frozen_package_frame(
    inputs: FrozenFrameInputs<'_>,
    seen_urls: &mut HashSet<String>,
) -> Option<Vec<u8>> {
    if !matches!(
        inputs.snapshot.resolution,
        LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
    ) {
        return None;
    }
    // The frame carries the integrity the client prefetches against;
    // an entry that pins none has no frame to announce.
    let Ok((upstream_tarball_url, Some(integrity))) =
        tarball_url_and_integrity(&inputs.snapshot.resolution, inputs.package_key, inputs.config)
    else {
        return None;
    };
    let name = inputs.package_key.name.to_string();
    let version = inputs.package_key.suffix.version().to_string();
    let tarball_url = inputs.router.route_url(&name, &version, &upstream_tarball_url);
    if !seen_urls.insert(tarball_url.clone()) {
        return None;
    }
    let id = format!("{name}@{version}");
    let revision =
        pnpr_served_revision(&inputs.snapshot.resolution, &tarball_url, &upstream_tarball_url);
    let stats = frozen_dist_stats(inputs.dist_stats, &name, &version);
    let frame = package_frame(
        inputs.router,
        &ResolvedPackageHint {
            integrity: &integrity.to_string(),
            tarball_url: &tarball_url,
            unpacked_size: stats.unpacked_size,
            file_count: stats.file_count,
            revision,
            requires_build: stats.requires_build,
            // The URL is already routed (canonical → endpoint above), so
            // re-routing by registry would be redundant; route_url is a
            // no-op on an already-routed URL.
            from_registry: false,
            identity: ResolvedPackageIdentity { id: &id, name: &name, version: &version },
        },
    );
    ndjson_line(&frame).ok()
}

fn frozen_dist_stats(stats: &ObservedDistStats, name: &str, version: &str) -> DistStats {
    stats
        .get(&(name.to_string(), version.to_string()))
        .map(|entry| *entry)
        .unwrap_or_default()
}

/// The tarball revision a frame announces. Only a URL still pointing at the
/// upstream carries one: a routed URL is served by pnpr, which addresses the
/// tarball by integrity rather than by revision.
fn pnpr_served_revision(
    resolution: &LockfileResolution,
    tarball_url: &str,
    upstream_tarball_url: &str,
) -> Option<u64> {
    if tarball_url != upstream_tarball_url {
        return None;
    }
    match resolution {
        LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
        LockfileResolution::Registry(registry) => registry.revision.map(TarballRevision::get),
        _ => None,
    }
}
