//! Detecting dependency specs that point at the local filesystem.

use super::{
    CatalogResolutionResult, Catalogs, Config, IncludedDependencies, PackageManifest, PathBuf,
    WantedDependency, resolve_from_catalog,
};

/// Whether any project declares a dependency with a local file
/// specifier in `dependencies`, `devDependencies`, or
/// `optionalDependencies`. Groups excluded from the current install
/// (per `included`) are skipped. `catalog:` specs are dereferenced
/// through the workspace catalogs.
pub(crate) fn has_local_file_dep(
    project_manifests: &[(PathBuf, &PackageManifest)],
    included: IncludedDependencies,
    catalogs: &Catalogs,
) -> bool {
    let fields: [(&str, bool); 3] = [
        ("dependencies", included.dependencies),
        ("devDependencies", included.dev_dependencies),
        ("optionalDependencies", included.optional_dependencies),
    ];
    project_manifests.iter().any(|(_, manifest)| {
        fields.iter().any(|(field, group_included)| {
            *group_included
                && manifest.value().get(*field).and_then(|value| value.as_object()).is_some_and(
                    |deps| {
                        deps.iter().any(|(alias, spec)| {
                            spec.as_str().is_some_and(|spec| {
                                is_local_file_spec(spec)
                                    || catalog_resolves_to_local_file(catalogs, alias, spec)
                            })
                        })
                    },
                )
        })
    })
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
/// the `file:` protocol, path-prefixed specs (`./`, `../`, `~/`,
/// absolute POSIX paths, and Windows drive paths including
/// drive-relative ones like `c:dir`), and bare tarball file names.
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
    if spec.contains(':') {
        return false;
    }
    if spec.contains('#') {
        return false;
    }
    ends_with_ignore_ascii_case(spec, ".tgz")
        || ends_with_ignore_ascii_case(spec, ".tar.gz")
        || ends_with_ignore_ascii_case(spec, ".tar")
}

/// Case-insensitive (ASCII) suffix check that, unlike
/// `spec.to_ascii_lowercase().ends_with(suffix)`, does not allocate.
pub(crate) fn ends_with_ignore_ascii_case(spec: &str, suffix: &str) -> bool {
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
