//! Detecting dependency specs that point at the local filesystem.

use super::{
    CatalogResolutionResult, Catalogs, Config, DependencyGroup, IncludedDependencies, Lockfile,
    OptimisticRepeatInstallCheck, Path, PathBuf, WantedDependency, resolve_from_catalog,
};
use pnpm_lockfile::{LockfileResolution, PkgName};
use pnpm_resolving_local_resolver::local_tarball_path;
use pnpm_workspace::importer_id_from_root_dir;
use ssri::{Integrity, IntegrityChecker};
use std::{borrow::Cow, fs, io::Read};

struct LocalTarballDependency {
    project_dir: PathBuf,
    alias: String,
    group: DependencyGroup,
    path: Option<PathBuf>,
    must_be_local: bool,
}

/// Whether any project declares a mutable local directory dependency or a
/// local tarball whose current bytes do not match the integrity recorded by
/// the previous install. Groups excluded from the current install are skipped.
/// `catalog:` specs are dereferenced through the workspace catalogs.
pub(crate) fn has_local_file_dep_requiring_install(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<bool, &'static str> {
    let fields: [(&str, DependencyGroup, bool); 3] = [
        ("dependencies", DependencyGroup::Prod, check.included.dependencies),
        ("devDependencies", DependencyGroup::Dev, check.included.dev_dependencies),
        ("optionalDependencies", DependencyGroup::Optional, check.included.optional_dependencies),
    ];
    let mut tarballs = Vec::new();
    for (project_dir, manifest) in check.project_manifests {
        for (field, group, group_included) in fields {
            if !group_included {
                continue;
            }
            let Some(deps) = manifest.value().get(field).and_then(|value| value.as_object()) else {
                continue;
            };
            for (alias, spec) in deps {
                let Some(spec) = spec.as_str() else { continue };
                let resolved_spec = resolve_catalog_spec(check.catalogs, alias, spec);
                let Some(spec) = resolved_spec.as_deref() else { continue };
                if !is_local_file_spec(spec) {
                    continue;
                }
                let must_be_local = is_unambiguous_local_file_spec(spec);
                let path = local_tarball_path(spec, project_dir);
                if must_be_local && path.is_none() {
                    return Ok(true);
                }
                tarballs.push(LocalTarballDependency {
                    project_dir: project_dir.clone(),
                    alias: alias.clone(),
                    group,
                    path,
                    must_be_local,
                });
            }
        }
    }
    if tarballs.is_empty() {
        return Ok(false);
    }

    let current_lockfile;
    let lockfile = if let Some(lockfile) = check
        .lockfile
        .get()
        .map_err(|_| "the wanted lockfile cannot be loaded to verify local tarballs")?
    {
        lockfile
    } else {
        current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir)
                .map_err(|_| "the current lockfile cannot be loaded to verify local tarballs")?;
        let Some(lockfile) = current_lockfile.as_ref() else { return Ok(true) };
        lockfile
    };

    Ok(tarballs.iter().any(|dependency| {
        local_tarball_requires_install(check.workspace_root, lockfile, dependency)
    }))
}

fn resolve_catalog_spec<'a>(
    catalogs: &Catalogs,
    alias: &str,
    spec: &'a str,
) -> Option<Cow<'a, str>> {
    if !spec.starts_with("catalog:") {
        return Some(Cow::Borrowed(spec));
    }
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
    ) {
        CatalogResolutionResult::Found(found) => Some(Cow::Owned(found.resolution.specifier)),
        _ => None,
    }
}

fn local_tarball_requires_install(
    workspace_root: &Path,
    lockfile: &Lockfile,
    dependency: &LocalTarballDependency,
) -> bool {
    let importer_id = importer_id_from_root_dir(workspace_root, &dependency.project_dir);
    let Some(importer) = lockfile.importers.get(&importer_id) else { return true };
    let Ok(alias) = PkgName::parse(&dependency.alias) else { return true };
    let Some(resolved) = importer
        .get_map_by_group(dependency.group)
        .and_then(|dependencies| dependencies.get(&alias))
    else {
        return true;
    };
    let Some(package_key) = resolved.version.resolved_key(&alias).map(|key| key.without_peer())
    else {
        return dependency.must_be_local;
    };
    let Some(metadata) = lockfile.packages.as_ref().and_then(|packages| packages.get(&package_key))
    else {
        return true;
    };
    let LockfileResolution::Tarball(resolution) = &metadata.resolution else {
        return dependency.must_be_local;
    };
    if !resolution.tarball.starts_with("file:") {
        return dependency.must_be_local;
    }
    let Some(recorded_path) = local_tarball_path(&resolution.tarball, workspace_root) else {
        return true;
    };
    if dependency.path.as_ref().is_some_and(|path| path != &recorded_path) {
        return true;
    }
    let Some(integrity) = resolution.integrity.as_ref().filter(|value| !value.hashes.is_empty())
    else {
        return true;
    };
    !file_matches_integrity(&recorded_path, integrity)
}

fn file_matches_integrity(path: &Path, integrity: &Integrity) -> bool {
    let Ok(mut file) = fs::File::open(path) else { return false };
    let Ok(metadata) = file.metadata() else { return false };
    if !metadata.is_file() {
        return false;
    }
    let mut checker = IntegrityChecker::new(integrity.clone());
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return checker.result().is_ok(),
            Ok(read) => checker.input(&buffer[..read]),
            Err(_) => return false,
        }
    }
}

/// Whether a `catalog:` spec dereferences (through the workspace
/// catalogs) to a local file specifier. A misconfigured catalog entry
/// returns `false`: it fails the full install with the proper error
/// anyway, so the fast path only needs to not report up-to-date for a
/// *valid* catalog entry holding a local path.
pub(crate) fn catalog_resolves_to_local_file(catalogs: &Catalogs, alias: &str, spec: &str) -> bool {
    // `resolve_from_catalog` returns `Unused` for any non-`catalog:` spec, so
    // short-circuit before allocating the owned `WantedDependency` it needs.
    if !spec.starts_with("catalog:") {
        return false;
    }
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
    ) {
        CatalogResolutionResult::Found(found) => is_local_file_spec(&found.resolution.specifier),
        _ => false,
    }
}

/// Whether any `pnpm.overrides` entry maps to a local file specifier.
/// An override redirects every matching dependency in the graph to its
/// specifier, so a local file override makes the installed contents
/// depend on that directory or tarball the same way a direct local file
/// dependency does. A parse failure returns its own distinct reason —
/// not the local-file reason, which would misattribute the cause.
pub(crate) fn has_local_file_override(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<bool, &'static str> {
    match crate::install::parse_config_overrides(config, catalogs) {
        Ok(Some(overrides)) => {
            Ok(overrides.iter().any(|entry| is_local_file_spec(&entry.new_bare_specifier)))
        }
        Ok(None) => Ok(false),
        Err(_) => Err("pnpm.overrides cannot be parsed"),
    }
}

/// Whether any `packageExtensions` entry injects a dependency with a
/// local file specifier. Package extensions are merged into matching
/// packages' manifests by the read-package hook during the full
/// install, so a `file:`/local-path/tarball spec added there has the
/// same content-change blind spot as a direct local file dependency
/// without appearing in any project manifest. Only `dependencies` and
/// `optionalDependencies` are scanned: peer dependencies are resolved
/// from the graph rather than fetched, so a local spec there is never
/// installed.
pub(crate) fn has_local_file_package_extension(
    config: &Config,
    included: IncludedDependencies,
    catalogs: &Catalogs,
) -> bool {
    let Some(extensions) = config.package_extensions.as_ref() else {
        return false;
    };
    extensions.values().any(|extension| {
        let optional = included
            .optional_dependencies
            .then_some(extension.optional_dependencies.as_ref())
            .flatten();
        [extension.dependencies.as_ref(), optional].into_iter().flatten().any(|deps| {
            deps.iter().any(|(alias, spec)| {
                is_local_file_spec(spec) || catalog_resolves_to_local_file(catalogs, alias, spec)
            })
        })
    })
}

/// Whether the specifier resolves to a local directory or tarball whose
/// contents can change without any manifest or lockfile mtime moving:
/// the `file:` protocol and path-prefixed specs (`./`, `../`, `~/`,
/// absolute POSIX paths, and Windows drive paths including
/// drive-relative ones like `c:dir`).
///
/// Deliberately narrower than the local resolver's bare-path matching:
/// a bare path like `user/repo` is statically indistinguishable from a
/// git shorthand at this layer, and matching it would disable the
/// repeat-install fast path for every project with git dependencies.
/// Such specs (and anything else carrying a protocol or URL) stay on
/// the fast path. `catalog:` specs also return `false` here — callers
/// dereference them through the workspace catalogs first, because a
/// catalog entry may hold a bare local path (the catalog resolver only
/// bans the `workspace:`, `link:`, and `file:` protocols).
pub(crate) fn is_local_file_spec(spec: &str) -> bool {
    if is_unambiguous_local_file_spec(spec) {
        return true;
    }
    if spec.contains([':', '#']) {
        return false;
    }
    ends_with_ignore_ascii_case(spec, ".tgz")
        || ends_with_ignore_ascii_case(spec, ".tar.gz")
        || ends_with_ignore_ascii_case(spec, ".tar")
}

fn is_unambiguous_local_file_spec(spec: &str) -> bool {
    if spec.starts_with("file:") {
        return true;
    }
    if spec.starts_with(['.', '/', '\\'])
        || spec.starts_with("~/")
        || spec.starts_with(r"~\")
        || is_windows_drive_path(spec)
    {
        return true;
    }
    false
}

fn ends_with_ignore_ascii_case(spec: &str, suffix: &str) -> bool {
    let spec = spec.as_bytes();
    let suffix = suffix.as_bytes();
    spec.len() >= suffix.len() && spec[spec.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// `c:/...`, `c:\...`, or drive-relative `c:foo` — a Windows drive
/// path. No separator is required after the colon; no registry protocol
/// is a single letter, so `[a-z]:` is unambiguous.
pub(crate) fn is_windows_drive_path(spec: &str) -> bool {
    let bytes = spec.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
