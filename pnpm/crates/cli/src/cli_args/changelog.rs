//! `registry`-storage changelog composition and the publication check that
//! gates intent garbage-collection. The registry access the pure
//! `pacquet-versioning` crate deliberately lacks lives here, in the CLI, which
//! already builds a registry client for publish. Mirrors the TypeScript
//! `releasing/commands/src/publish/previousChangelog.ts`.

use std::{collections::HashSet, io::Read, path::Path};

use flate2::read::GzDecoder;
use futures_util::StreamExt;
use miette::IntoDiagnostic;
use pacquet_config::Config;
use pacquet_network::{ThrottledClient, encode_package_name, redact_url_credentials};
use pacquet_registry::Package;
use pacquet_versioning::{
    ChangelogStorage, ReleasePlan, changelog_storage, list_pending_changelogs,
    read_pending_changelog, render_changelog,
};
use tar::Archive;

use crate::cli_args::registry_client::build_registry_client;

const CHANGELOG_ENTRY: &str = "package/CHANGELOG.md";

/// Caps the previous tarball we buffer and decompress to compose the changelog.
/// The bytes come from a registry/proxy, so an unbounded read or a highly
/// compressible ("gzip bomb") tarball could OOM release automation. A composed
/// changelog is best-effort — exceeding the cap just skips the history prepend —
/// so this bound can be generous while still defeating the amplification attack.
const MAX_TARBALL_BYTES: u64 = 256 * 1024 * 1024;

/// The composed CHANGELOG.md to pack for a `registry`-storage release of the
/// project at `project_dir`: its parked section rendered on top of the
/// previously published version's changelog. `None` unless storage is
/// `registry` and the release has a parked section (an ordinary `pnpm pack`
/// of a project that is not mid-release).
pub async fn compose_registry_changelog(
    config: &Config,
    project_dir: &Path,
) -> miette::Result<Option<Vec<u8>>> {
    if changelog_storage(Some(&config.versioning)) != ChangelogStorage::Registry {
        return Ok(None);
    }
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(None);
    };
    let Some((name, version)) = read_name_version(project_dir) else {
        return Ok(None);
    };
    let Some(section) = read_pending_changelog(workspace_dir, &name, &version)? else {
        return Ok(None);
    };
    let previous = fetch_changelog(config, &name, VersionPick::PreviousTo(&version)).await;
    Ok(Some(render_changelog(previous.as_deref(), &name, &section).into_bytes()))
}

/// The set of `package@version` keys the registry confirms are published with
/// their parked changelog section — the gate `apply_release_plan` uses to
/// garbage-collect consumed intents and parked sections in `registry` storage.
/// Driven by the parked files rather than the ledger, so a dependency-propagated
/// release (which has a section but no consumed intents, hence no ledger entry)
/// is confirmed too. Every parked file belongs to an as-yet-unpublished
/// release, so the cost is bounded by the release backlog, not by history. The
/// checks run concurrently. Empty in `repository` storage.
pub async fn confirmed_published_versions(
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<HashSet<String>> {
    if changelog_storage(Some(&config.versioning)) != ChangelogStorage::Registry {
        return Ok(HashSet::new());
    }
    let checks =
        list_pending_changelogs(workspace_dir)?.into_iter().map(|(name, version)| async move {
            let section = read_pending_changelog(workspace_dir, &name, &version).ok()??;
            let changelog = fetch_changelog(config, &name, VersionPick::Exact(&version)).await?;
            changelog.contains(section.trim()).then(|| format!("{name}@{version}"))
        });
    Ok(futures_util::future::join_all(checks).await.into_iter().flatten().collect())
}

/// The directories in `plan` whose current manifest version is not yet
/// published to its registry — the packages whose first release must publish
/// that version verbatim instead of bumping off it. Probes every release
/// concurrently; any probe failure propagates so the command fails rather than
/// release a wrong version. Feeds `AssembleReleasePlanOptions::unpublished_dirs`.
/// Mirrors the TypeScript `resolveUnpublishedDirs`.
///
/// A first assembly pass (without `unpublished_dirs`) supplies `plan`. Holding
/// a package at its current version can only remove dependent propagation,
/// never add it (the materialized range already admits the unchanged version),
/// so the re-assembled plan is a subset of `plan` — every dir it can hold was
/// probed here.
pub async fn unpublished_release_dirs(
    config: &Config,
    plan: &ReleasePlan,
) -> miette::Result<HashSet<String>> {
    // Debug-only test seam: the engine's integration tests advance manifests
    // without a real publish cycle (a lane prerelease is written but never
    // published, for instance), so they set this to make every release bump as
    // if already published. Compiled out of release builds, so a production
    // `pnpm` always probes.
    #[cfg(debug_assertions)]
    if std::env::var_os("PACQUET_ASSUME_VERSIONS_PUBLISHED").is_some() {
        return Ok(HashSet::new());
    }
    // One client, shared across the concurrent probes, so the global
    // network-concurrency bound and connection pool are respected rather than
    // reconstructed per release: `is_version_published` awaits the client's
    // per-origin semaphore (`acquire_for_url`), which caps how many probes hit
    // the registry at once even though every future is spawned up front.
    let client = build_registry_client(config)?;
    let checks = plan.releases.iter().map(|release| {
        let client = &client;
        async move {
            let published =
                is_version_published(client, config, &release.name, &release.current_version)
                    .await?;
            Ok::<_, miette::Report>((release.dir.clone(), published))
        }
    });
    let probed = futures_util::future::try_join_all(checks).await?;
    Ok(probed.into_iter().filter_map(|(dir, published)| (!published).then_some(dir)).collect())
}

/// Whether `name@version` is already published to its registry. A registry 404
/// (no such package) and a package published but missing this exact version
/// both read as `false` — a never-published version, i.e. a first release. Any
/// other outcome (offline, 5xx, malformed body) errors, so the caller fails
/// rather than guess a version's fate.
async fn is_version_published(
    client: &ThrottledClient,
    config: &Config,
    name: &str,
    version: &str,
) -> miette::Result<bool> {
    let registry = registry_for(config, name);
    let url = format!("{registry}{}", encode_package_name(name));
    let guard = client.acquire_for_url(&url).await;
    let mut request = guard.get(&url).header(
        "accept",
        "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
    );
    if let Some(value) = config.auth_headers.for_url_with_package(&url, Some(name)) {
        request = request.header("authorization", value);
    }
    let response = request.send().await.into_diagnostic()?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Ok(false);
    }
    if !status.is_success() {
        let display_url = redact_url_credentials(&url);
        return Err(miette::miette!(
            "registry returned status {status} for {display_url} while checking whether {name}@{version} is published",
        ));
    }
    let package: Package = response.json().await.into_diagnostic()?;
    Ok(package.versions.contains_key(version))
}

enum VersionPick<'a> {
    Exact(&'a str),
    PreviousTo(&'a str),
}

/// Best-effort fetch of a published version's `CHANGELOG.md` — `None` when the
/// package or version is not published or its tarball carried no changelog.
/// Errors resolve to `None`: for the previous-version prepend that just starts
/// a fresh changelog, and for the publication check that keeps the intent.
async fn fetch_changelog(config: &Config, name: &str, pick: VersionPick<'_>) -> Option<String> {
    let client = build_registry_client(config).ok()?;
    let registry = registry_for(config, name);
    let package =
        Package::fetch_from_registry(name, &client, &registry, &config.auth_headers).await.ok()?;
    let version = match pick {
        VersionPick::Exact(version) => {
            package.versions.contains_key(version).then(|| version.to_string())?
        }
        VersionPick::PreviousTo(version) => previous_version(&package, version)?,
    };
    let tarball_url = package.versions.get(&version)?.as_tarball_url().to_string();
    let guard = client.acquire_for_url(&tarball_url).await;
    let mut request = guard.get(&tarball_url);
    if let Some(value) = config.auth_headers.for_url_with_package(&tarball_url, Some(name)) {
        request = request.header("authorization", value);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    // Bound the actual download rather than trusting `content-length` (which may
    // be absent or lie): stream the body and stop once it exceeds the cap.
    let mut stream = response.bytes_stream();
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if body.len() as u64 + chunk.len() as u64 > MAX_TARBALL_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    extract_entry(&body, CHANGELOG_ENTRY)
}

/// Highest published version of the package that is semver-lower than `version`.
fn previous_version(package: &Package, version: &str) -> Option<String> {
    let target: node_semver::Version = version.parse().ok()?;
    package
        .versions
        .keys()
        .filter_map(|key| key.parse::<node_semver::Version>().ok().map(|parsed| (parsed, key)))
        .filter(|(parsed, _)| *parsed < target)
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, key)| key.clone())
}

/// The registry a package's metadata is read from: the scope's registry when
/// configured, else the default. Mirrors `pickRegistryForPackage`.
fn registry_for(config: &Config, name: &str) -> String {
    let registry = name
        .strip_prefix('@')
        .and_then(|rest| rest.split('/').next())
        .and_then(|scope| config.registries.get(&format!("@{scope}")))
        .cloned()
        .unwrap_or_else(|| config.registry.clone());
    if registry.ends_with('/') { registry } else { format!("{registry}/") }
}

/// Reads one entry's contents out of a gzipped tarball buffer. Decompression is
/// capped (see [`MAX_TARBALL_BYTES`]) by reading at most that many inflated
/// bytes; a gzip bomb truncates past the cap and simply yields no entry.
fn extract_entry(gzipped_tarball: &[u8], entry_name: &str) -> Option<String> {
    let mut archive = Archive::new(GzDecoder::new(gzipped_tarball).take(MAX_TARBALL_BYTES));
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        if entry.path().ok()?.to_str() == Some(entry_name) {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).ok()?;
            return Some(contents);
        }
    }
    None
}

fn read_name_version(project_dir: &Path) -> Option<(String, String)> {
    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(project_dir.join("package.json"))
            .ok()?;
    let value = manifest.value();
    let name = value.get("name")?.as_str()?.to_string();
    let version = value.get("version")?.as_str()?.to_string();
    Some((name, version))
}
