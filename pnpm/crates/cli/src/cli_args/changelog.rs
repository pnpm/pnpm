//! `registry`-storage changelog composition and the publication check that
//! gates intent garbage-collection. The registry access the pure
//! `pacquet-versioning` crate deliberately lacks lives here, in the CLI, which
//! already builds a registry client for publish. Mirrors the TypeScript
//! `releasing/commands/src/publish/previousChangelog.ts`.

use std::{collections::HashSet, io::Read, path::Path};

use flate2::read::GzDecoder;
use pacquet_config::Config;
use pacquet_registry::Package;
use pacquet_versioning::{
    ChangeIntent, ChangelogStorage, Ledger, changelog_storage, read_pending_changelog,
    render_changelog,
};
use tar::Archive;

use crate::cli_args::registry_client::build_registry_client;

const CHANGELOG_ENTRY: &str = "package/CHANGELOG.md";

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

/// The set of `package@version` ledger keys the registry confirms are
/// published with their parked changelog section — the gate `apply_release_plan`
/// uses to garbage-collect consumed intents in `registry` storage. Only entries
/// still referenced by an unconsumed intent are checked, so already-collected
/// history costs no network. Empty in `repository` storage.
pub async fn confirmed_published_versions(
    config: &Config,
    workspace_dir: &Path,
    ledger: &Ledger,
    intents: &[ChangeIntent],
) -> miette::Result<HashSet<String>> {
    let mut confirmed = HashSet::new();
    if changelog_storage(Some(&config.versioning)) != ChangelogStorage::Registry {
        return Ok(confirmed);
    }
    let pending_ids: HashSet<&str> = intents.iter().map(|intent| intent.id.as_str()).collect();
    for (key, entry) in ledger {
        if !entry.intent_ids().iter().any(|id| pending_ids.contains(id.as_str())) {
            continue;
        }
        let Some((name, version)) = split_ledger_key(key) else {
            continue;
        };
        let Some(section) = read_pending_changelog(workspace_dir, name, version)? else {
            continue;
        };
        if let Some(changelog) = fetch_changelog(config, name, VersionPick::Exact(version)).await
            && changelog.contains(section.trim())
        {
            confirmed.insert(key.clone());
        }
    }
    Ok(confirmed)
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
    let bytes = response.bytes().await.ok()?;
    extract_entry(&bytes, CHANGELOG_ENTRY)
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

/// Reads one entry's contents out of a gzipped tarball buffer.
fn extract_entry(gzipped_tarball: &[u8], entry_name: &str) -> Option<String> {
    let mut archive = Archive::new(GzDecoder::new(gzipped_tarball));
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

/// Splits a `package@version` ledger key. The leading `@` of a scoped name is
/// not a separator, so the split is on the last `@`.
fn split_ledger_key(key: &str) -> Option<(&str, &str)> {
    let at = key.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some((&key[..at], &key[at + 1..]))
}
