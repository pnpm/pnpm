//! Warming the store from a package tarball on disk, such as one a
//! sandboxed build downloaded before it lost network access.

use super::{AddOne, ArchiveToAdd, Reporter, lockfile_entries::LockfileEntries};
use miette::{IntoDiagnostic, WrapErr};
use pnpm_local_spec::{is_filespec, is_tarball_filename};
use pnpm_resolving_local_resolver::local_spec_path;
use pnpm_store_dir::store_index_key;
use pnpm_tarball::local_file_tarball_path;
use rustc_hash::FxHashSet;
use std::{
    io,
    path::{Path, PathBuf},
};

/// The `resolved_via` of a `file:` tarball the local resolver resolved.
const LOCAL_FILESYSTEM: &str = "local-filesystem";

/// The tarball on disk a resolution names, when the local resolver made it.
///
/// A registry resolution can carry a `file:` tarball URL too, but it pins its
/// own integrity, which the registry ingestion path verifies. The local
/// resolver reads a relative path from `lockfile_dir`, which `--dir` can set
/// apart from the working directory, so the path is joined to it.
pub(super) fn tarball_on_disk(
    resolved_via: &str,
    package_url: &str,
    lockfile_dir: &Path,
) -> Option<PathBuf> {
    if resolved_via != LOCAL_FILESYSTEM {
        return None;
    }
    local_file_tarball_path(package_url).map(|path| lockfile_dir.join(path))
}

/// Expand any specifier in `packages` that names a directory of tarballs on
/// disk into `file:` specifiers for the tarballs it holds. A directory with a
/// root `package.json` or with no tarballs is left as-is so the resolver
/// reports it as an unsupported local package specification.
pub(super) fn expand_tarball_directories(
    dir: &Path,
    packages: &[String],
) -> miette::Result<Vec<String>> {
    let mut expanded = Vec::with_capacity(packages.len());
    for package in packages {
        match tarballs_in_directory(dir, package)? {
            Some(tarballs) => expanded.extend(tarballs),
            None => expanded.push(package.clone()),
        }
    }
    Ok(expanded)
}

fn tarballs_in_directory(dir: &Path, package: &str) -> miette::Result<Option<Vec<String>>> {
    let Some(root) = tarball_directory_path(dir, package) else {
        return Ok(None);
    };
    let tarballs = collect_tarballs_in_dir(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("read tarball directory {}", root.display()))?;
    Ok((!tarballs.is_empty()).then(|| {
        tarballs
            .into_iter()
            .map(|path| format!("file:{}", path.display()))
            .collect()
    }))
}

fn tarball_directory_path(dir: &Path, package: &str) -> Option<PathBuf> {
    if package.starts_with("file:") || is_filespec(package) {
        let path = local_spec_path(package, dir);
        return is_tarball_directory(&path).then_some(path);
    }
    None
}

fn is_tarball_directory(path: &Path) -> bool {
    path.is_dir() && !path.join("package.json").exists()
}

fn collect_tarballs_in_dir(root: &Path) -> io::Result<Vec<PathBuf>> {
    let canonical_root = dunce::canonicalize(root)?;
    let mut tarballs = Vec::new();
    let mut visited_dirs = FxHashSet::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let canonical = dunce::canonicalize(&dir)?;
        if !canonical.starts_with(&canonical_root) || !visited_dirs.insert(canonical) {
            continue;
        }
        visit_tarball_dir(&dir, &canonical_root, &mut stack, &mut tarballs)?;
    }
    Ok(tarballs)
}

fn visit_tarball_dir(
    dir: &Path,
    canonical_root: &Path,
    stack: &mut Vec<PathBuf>,
    tarballs: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<PathBuf>>>()?;
    entries.sort();
    let mut subdirs = Vec::new();
    for path in entries {
        if path.is_dir() {
            subdirs.push(path);
        } else if path.is_file()
            && is_tarball_path(&path)
            && dunce::canonicalize(&path)?.starts_with(canonical_root)
        {
            tarballs.push(path);
        }
    }
    stack.extend(subdirs.into_iter().rev());
    Ok(())
}

fn is_tarball_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_tarball_filename)
}

impl AddOne<'_> {
    /// Ingest the tarball at `path`, then make its row reachable under every
    /// key an install of the registry package it holds may look up:
    /// `<integrity>\t<name>@<version>` for the tarball's sha512 and sha1 and
    /// for the tar it holds. Which one a lockfile records depends on the
    /// package's age and on `lockfile.integrityTarget`. A package from a
    /// registry reached through a prefix may be keyed
    /// `<name>@<prefix>:<version>`, and the tarball does not say which
    /// registry it came from, so every declared and built-in prefix gets
    /// aliases. Each entry of the project's lockfile that pins one of those
    /// hashes gets its key too, which covers ids no manifest reveals, such as
    /// a tarball URL.
    ///
    /// Each alias names a hash of the bytes that were ingested, so a lockfile
    /// entry reaches the row only when it pins exactly this tarball.
    pub(super) async fn add_downloaded_tarball<Reporter: self::Reporter>(
        &self,
        path: &Path,
        archive: &ArchiveToAdd<'_>,
    ) -> miette::Result<()> {
        let lockfile_entries = self.lockfile_entries().await?;
        let fetched = self
            .ingestion(archive, None)
            .ingest_local_tarball::<Reporter>(path)
            .await
            .map_err(miette::Report::new)?;
        let tarball = &fetched.tarball;
        let row_key = store_index_key(&tarball.integrity.to_string(), archive.id);
        let integrities = [
            Some(&tarball.integrity),
            Some(&fetched.sha1_integrity),
            tarball.tar_integrity.as_ref(),
        ];
        let prefixes = self.store.registry_prefixes.iter().map(String::as_str);
        let registry_aliases = registry_package_ids(tarball.manifest.as_ref(), prefixes)
            .into_iter()
            .flat_map(|registry_id| {
                integrities
                    .iter()
                    .flatten()
                    .map(move |integrity| store_index_key(&integrity.to_string(), &registry_id))
            });
        let lockfile_aliases = lockfile_entries
            .store_index_keys(integrities.into_iter().flatten())
            .map(str::to_owned);
        registry_aliases
            .chain(lockfile_aliases)
            .filter(|alias| *alias != row_key)
            .collect::<FxHashSet<_>>()
            .into_iter()
            .for_each(|alias| self.store.index_writer.queue_alias(alias, row_key.clone()));
        Ok(())
    }

    async fn lockfile_entries(&self) -> miette::Result<&LockfileEntries> {
        self.lockfile_entries.get_or_try_init(|| async {
            LockfileEntries::load(self.config, &self.resolve_options.project.project_dir)
        })
        .await
    }
}

/// The ids a registry resolution of the package this manifest names may
/// carry: `<name>@<version>` with the version as the manifest spells it and,
/// when that differs, as semver writes it (`v1.0.0` is `1.0.0`). The store's
/// package content check accepts either spelling. Each of `prefixes` adds
/// `<name>@<prefix>:<version>`, with the version as semver writes it.
fn registry_package_ids<'p>(
    manifest: Option<&serde_json::Value>,
    prefixes: impl IntoIterator<Item = &'p str>,
) -> Vec<String> {
    let Some((name, version)) = manifest.and_then(|manifest| {
        Some((manifest.get("name")?.as_str()?, manifest.get("version")?.as_str()?))
    }) else {
        return Vec::new();
    };
    let mut ids = vec![format!("{name}@{version}")];
    let Ok(parsed) = node_semver::Version::parse(version) else { return ids };
    if parsed.to_string() != version {
        ids.push(format!("{name}@{parsed}"));
    }
    ids.extend(
        prefixes
            .into_iter()
            .map(|prefix| format!("{name}@{prefix}:{parsed}")),
    );
    ids
}

#[cfg(test)]
mod tests;
