//! The tools a snapshot's `buildDependencies` record (`lockBuildTools`).

use crate::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, Prefix, SnapshotEntry,
    TarballResolution,
};
use pnpm_tool_protocol::{PackageManagerSource, package_manager_source};
use std::collections::HashMap;

/// The artifact a `tool:` entry is: the npm tarball of the package the tool
/// is published as, or the platform archives of a tool that ships them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuildToolArtifact<'a> {
    /// `package` is the npm package (`@yarnpkg/cli-dist` for Yarn 2 to 5).
    Registry { package: &'static str, tarball: &'a TarballResolution },
    /// Node.js, Bun, and Yarn 6 and later.
    Archive,
}

/// The artifact the `tool:` entry `key` is, when `resolution` has the shape
/// the tool's source gives it. `None` for anything else: a key without the
/// `tool:` prefix, a tool pnpm does not provision, a tarball that is not an
/// integrity-pinned http(s) download, or a registry resolution, whose URL
/// would be derived from the tool's name rather than from its package.
#[must_use]
pub fn build_tool_artifact<'a>(
    key: &PackageKey,
    resolution: &'a LockfileResolution,
) -> Option<BuildToolArtifact<'a>> {
    if key.suffix.prefix() != Prefix::Tool {
        return None;
    }
    let name = key.name.to_string();
    let source = if name == "node" {
        PackageManagerSource::Archive
    } else {
        package_manager_source(&name, &key.suffix.version().to_string())?
    };
    match (source, resolution) {
        (PackageManagerSource::Registry(package), LockfileResolution::Tarball(tarball))
            if is_pinned_http_tarball(tarball) =>
        {
            Some(BuildToolArtifact::Registry { package, tarball })
        }
        (PackageManagerSource::Archive, archive) if is_downloaded_archive(archive) => {
            Some(BuildToolArtifact::Archive)
        }
        _ => None,
    }
}

fn is_pinned_http_tarball(tarball: &TarballResolution) -> bool {
    tarball.integrity.is_some() && !tarball.is_git_hosted() && is_http_url(&tarball.tarball)
}

/// A binary archive, or platform archives, that are all downloads.
fn is_downloaded_archive(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Binary(binary) => is_http_url(&binary.url),
        LockfileResolution::Variations(variations) => {
            !variations.variants.is_empty()
                && variations.variants
                    .iter()
                    .all(|variant| is_downloaded_archive(&variant.resolution))
        }
        _ => false,
    }
}

fn is_http_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    url.starts_with("https://") || url.starts_with("http://")
}

impl crate::Lockfile {
    /// The `packages:` keys of the build tools that the snapshots at
    /// `snapshot_keys` record. A walk that keeps those snapshots keeps
    /// their tools' metadata too, though no install reaches the tools.
    pub fn build_tool_keys<'a>(
        &'a self,
        snapshot_keys: impl IntoIterator<Item = &'a PackageKey> + 'a,
    ) -> impl Iterator<Item = PackageKey> + 'a {
        snapshot_keys
            .into_iter()
            .filter_map(|key| self.snapshots.as_ref()?.get(key))
            .flat_map(|snapshot| {
                snapshot.build_dependency_keys().map(|(_, key)| key.without_peer())
            })
    }
}

impl SnapshotEntry {
    /// The `packages:` / `snapshots:` key of every tool this snapshot
    /// records, with the name it records the tool under.
    pub fn build_dependency_keys(&self) -> impl Iterator<Item = (&PkgName, PackageKey)> {
        self.build_dependencies
            .iter()
            .flatten()
            .filter_map(|(alias, dep_ref)| Some((alias, dep_ref.resolve(alias)?)))
    }
}

/// A digest naming every tool `snapshot` records by its key and resolution,
/// so two builds share it only when they ran on the same bytes. `None` when
/// the snapshot records no tools.
#[must_use]
pub fn build_tools_identity(
    snapshot: &SnapshotEntry,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Option<String> {
    let mut tools: Vec<String> = snapshot
        .build_dependency_keys()
        .map(|(alias, key)| {
            let resolution = packages
                .and_then(|packages| packages.get(&key))
                .and_then(|metadata| serde_json::to_string(&metadata.resolution).ok())
                .unwrap_or_default();
            format!("{alias}={key}={resolution}")
        })
        .collect();
    if tools.is_empty() {
        return None;
    }
    tools.sort_unstable();
    Some(pnpm_crypto_hash::create_hex_hash(&tools.join("\n")))
}

#[cfg(test)]
mod tests;
