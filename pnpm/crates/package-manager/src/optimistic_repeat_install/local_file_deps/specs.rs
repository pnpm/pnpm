use crate::optimistic_repeat_install::{
    CatalogAnchor, CatalogResolutionResult, Catalogs, Config, IncludedDependencies,
    WantedDependency, resolve_from_catalog,
};
use pnpm_config_parse_overrides::VersionOverride;
use pnpm_lockfile::is_local_tarball_path;

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
    // Only the shape of the entry decides this, and re-anchoring a
    // relative path never changes it, so the entry is read as written.
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
        CatalogAnchor::AsWritten,
    ) {
        CatalogResolutionResult::Found(found) => is_local_file_spec(&found.resolution.specifier),
        _ => false,
    }
}

/// Whether any override maps to a local file specifier.
pub(crate) fn has_local_file_override(overrides: &[VersionOverride]) -> bool {
    overrides.iter().any(|entry| is_local_file_spec(&entry.new_bare_specifier))
}

/// Whether the dependency's specifier is (or resolves through a
/// catalog to) a local file specifier and no generic (parentless)
/// override replaces it. Parent-scoped overrides (`parent>dep`)
/// never suppress the bail-out: whether they apply depends on which
/// package the dependency belongs to. Convergence overrides (`pkg@`)
/// never suppress the bail-out: they apply only to plain semver ranges.
pub(crate) fn is_effective_local_file_dep(
    catalogs: &Catalogs,
    overrides: &[VersionOverride],
    alias: &str,
    spec: &str,
) -> bool {
    (is_local_file_spec(spec) || catalog_resolves_to_local_file(catalogs, alias, spec))
        && !is_dep_replaced_by_override(overrides, alias, spec)
}

pub(crate) fn is_dep_replaced_by_override(
    overrides: &[VersionOverride],
    alias: &str,
    spec: &str,
) -> bool {
    overrides
        .iter()
        .any(|entry| {
            !entry.converge
                && entry.parent_pkg.is_none()
                && crate::overrides::matches_target(&entry.target_pkg, alias, spec)
        })
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
    overrides: &[VersionOverride],
) -> bool {
    let Some(extensions) = config.package_extensions.as_ref() else {
        return false;
    };
    extensions
        .values()
        .any(|extension| {
            let optional = included.optional_dependencies
                .then_some(extension.optional_dependencies.as_ref())
                .flatten();
            [extension.dependencies.as_ref(), optional]
                .into_iter()
                .flatten()
                .any(|deps| {
                    deps.iter()
                        .any(|(alias, spec)| {
                            is_effective_local_file_dep(catalogs, overrides, alias, spec)
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
/// catalog entry may itself hold a local path.
pub(crate) fn is_local_file_spec(spec: &str) -> bool {
    if is_unambiguous_local_file_spec(spec) {
        return true;
    }
    if spec.contains([':', '#']) {
        return false;
    }
    is_local_tarball_path(spec)
}

pub(crate) fn is_unambiguous_local_file_spec(spec: &str) -> bool {
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

/// `c:/...`, `c:\...`, or drive-relative `c:foo` — a Windows drive
/// path. No separator is required after the colon; no registry protocol
/// is a single letter, so `[a-z]:` is unambiguous.
pub(crate) fn is_windows_drive_path(spec: &str) -> bool {
    let bytes = spec.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
