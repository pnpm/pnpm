//! `catalogMode` reconciliation for `pacquet add` / `pacquet update`.
//!
//! Ports pnpm's catalog-mode gate in
//! [`installSome`](https://github.com/pnpm/pnpm/blob/6c65cb5c18/installing/deps-installer/src/install/index.ts#L793-L828):
//! a direct version written to the manifest is checked against the
//! matching `catalog:` entry, and — depending on [`CatalogMode`] — either
//! rejected, kept with a warning, or ignored.

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_resolver::{CatalogResolutionResult, WantedDependency, resolve_from_catalog};
use pacquet_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pacquet_config::CatalogMode;
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};

/// Wanted dependency outside the version range defined in catalog.
///
/// Ports pnpm's
/// [`CatalogVersionMismatchError`](https://github.com/pnpm/pnpm/blob/6c65cb5c18/installing/deps-installer/src/install/checkCompatibility/CatalogVersionMismatchError.ts).
/// Raised under [`CatalogMode::Strict`] when a direct `add` / `update`
/// version disagrees with the matching catalog entry.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("Wanted dependency outside the version range defined in catalog")]
#[diagnostic(code(ERR_PNPM_CATALOG_VERSION_MISMATCH))]
pub struct CatalogVersionMismatchError {
    /// `<name>@<catalog specifier>`, the version the catalog pins.
    pub catalog_dep: String,
    /// `<name>@<wanted specifier>`, the version the command asked for.
    pub wanted_dep: String,
}

/// A direct dependency `add` / `update` is about to write to the
/// manifest, paired with the specifier it currently carries.
pub struct CatalogModeDep<'a> {
    /// The dependency's package name.
    pub alias: &'a str,
    /// The new bare specifier being written (e.g. `1.0.0`, `^2`).
    pub bare_specifier: &'a str,
    /// The specifier already recorded in the manifest, consulted to
    /// preserve a named catalog group (`catalog:<name>`). `None` for a
    /// freshly-added dependency.
    pub prev_specifier: Option<&'a str>,
}

/// Reconcile each wanted dependency against the catalogs under the
/// configured [`CatalogMode`].
///
/// Returns `Err` (aborting the operation before any manifest mutation)
/// the first time [`CatalogMode::Strict`] finds a mismatch; emits a
/// `pnpm` warning per mismatch under [`CatalogMode::Prefer`]; does
/// nothing under [`CatalogMode::Manual`].
pub fn check_catalog_mode<Reporter: self::Reporter>(
    catalog_mode: CatalogMode,
    catalogs: &Catalogs,
    deps: &[CatalogModeDep<'_>],
    prefix: &str,
) -> Result<(), CatalogVersionMismatchError> {
    if catalog_mode == CatalogMode::Manual {
        return Ok(());
    }

    for dep in deps {
        // A `runtime:` specifier round-trips to `devEngines.runtime`
        // through the manifest writer; promoting it into a catalog would
        // strand it in `devDependencies`. Skip it, matching pnpm.
        if dep.bare_specifier.starts_with("runtime:") {
            continue;
        }

        let per_dep_catalog_name = per_dep_catalog_name(dep.prev_specifier);
        let catalog_bare_specifier = if per_dep_catalog_name == DEFAULT_CATALOG_NAME {
            "catalog:".to_string()
        } else {
            format!("catalog:{per_dep_catalog_name}")
        };

        // `pickCatalogSpecifier`: only a `Found` resolution yields a
        // specifier to reconcile against. `Unused` (no catalog entry) and
        // `Misconfiguration` (surfaced later, at install) are treated as
        // "nothing to compare", so the wanted version is kept as-is.
        let wanted = WantedDependency {
            alias: dep.alias.to_string(),
            bare_specifier: catalog_bare_specifier.clone(),
        };
        let CatalogResolutionResult::Found(found) = resolve_from_catalog(catalogs, &wanted) else {
            continue;
        };
        let catalog_specifier = found.resolution.specifier;

        // Already references the catalog, or names the exact same concrete
        // version → it agrees; keep it.
        if dep.bare_specifier == catalog_bare_specifier
            || versions_equal(dep.bare_specifier, &catalog_specifier)
        {
            continue;
        }

        match catalog_mode {
            CatalogMode::Strict => {
                return Err(CatalogVersionMismatchError {
                    catalog_dep: format!("{}@{catalog_specifier}", dep.alias),
                    wanted_dep: format!("{}@{}", dep.alias, dep.bare_specifier),
                });
            }
            CatalogMode::Prefer => {
                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Warn,
                    message: format!(
                        "Catalog version mismatch for \"{}\": using direct version \"{}\" instead of catalog version \"{catalog_specifier}\".",
                        dep.alias, dep.bare_specifier,
                    ),
                    prefix: prefix.to_string(),
                }));
            }
            CatalogMode::Manual => {}
        }
    }

    Ok(())
}

/// Equal only when **both** specifiers are concrete semver versions that
/// compare equal. A range (e.g. `^2.0.0`) fails [`Version::parse`], so it
/// never reaches the comparison — the Rust analogue of pnpm guarding
/// `semver.eq` with `semver.valid`
/// ([pnpm#11706](https://github.com/pnpm/pnpm/pull/11706)). Passing a
/// range to an exact-version comparison is the bug that fix prevents.
fn versions_equal(lhs: &str, rhs: &str) -> bool {
    matches!((Version::parse(lhs), Version::parse(rhs)), (Ok(left), Ok(right)) if left == right)
}

/// The catalog group a dependency belongs to: a previous `catalog:<name>`
/// specifier pins the named group, otherwise the default catalog. Mirrors
/// pnpm's
/// [`getPerDepCatalogName`](https://github.com/pnpm/pnpm/blob/6c65cb5c18/installing/deps-installer/src/install/index.ts#L1223-L1234)
/// without the global `saveCatalogName` (which pacquet has not ported).
fn per_dep_catalog_name(prev_specifier: Option<&str>) -> &str {
    prev_specifier.and_then(parse_catalog_protocol).unwrap_or(DEFAULT_CATALOG_NAME)
}

#[cfg(test)]
mod tests;
