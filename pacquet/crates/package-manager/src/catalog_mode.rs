//! `catalogMode` reconciliation for `pacquet add` / `pacquet update`.
//!
//! Ports both halves of pnpm's catalog-mode handling:
//!
//! - the **gate** in
//!   [`installSome`](https://github.com/pnpm/pnpm/blob/6c65cb5c18/installing/deps-installer/src/install/index.ts#L793-L828):
//!   a direct version disagreeing with a matching `catalog:` entry is
//!   rejected ([`CatalogMode::Strict`]) or kept with a warning
//!   ([`CatalogMode::Prefer`]);
//! - the **auto-cataloging** decision (`saveCatalogName` /
//!   `catalogLookup`, pnpm's
//!   [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/installing/deps-resolver/src/resolveDependencyTree.ts#L280-L304)):
//!   a matching or not-yet-cataloged dependency is rewritten to
//!   `catalog:` / `catalog:<name>` and, when no entry exists yet, recorded
//!   for write-back to `pnpm-workspace.yaml`.

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
    /// The new bare specifier being written (e.g. `1.0.0`, `^2`), or
    /// `catalog:` / `catalog:<name>` when the manifest already references a
    /// catalog and is staying that way.
    pub bare_specifier: &'a str,
    /// The specifier already recorded in the manifest, consulted to
    /// preserve a named catalog group (`catalog:<name>`). `None` for a
    /// freshly-added dependency.
    pub prev_specifier: Option<&'a str>,
}

/// A catalog entry to insert/update in `pnpm-workspace.yaml` (and mirror in
/// `pnpm-lock.yaml`'s `catalogs` snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Catalog group the entry belongs to (`default` or a named catalog).
    pub catalog_name: String,
    /// Version specifier to record under the catalog.
    pub specifier: String,
}

/// What to do with one dependency under the configured [`CatalogMode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogDecision {
    /// Keep the direct specifier; do not catalog this dependency.
    KeepDirect,
    /// Write `catalog:` / `catalog:<name>` to the manifest.
    Catalog {
        /// The specifier to write to the project's `package.json`.
        manifest_specifier: String,
        /// The catalog entry to write back to the workspace manifest, or
        /// `None` when an existing entry is reused unchanged.
        updated_entry: Option<CatalogEntry>,
    },
}

/// Decide how to reconcile one dependency against the catalogs under the
/// configured [`CatalogMode`] and `save_catalog_name`.
///
/// Returns `Err` (under [`CatalogMode::Strict`]) when the wanted version
/// disagrees with an existing catalog entry; emits a `pnpm` warning and
/// returns [`CatalogDecision::KeepDirect`] under [`CatalogMode::Prefer`].
pub fn decide_catalog<Reporter: self::Reporter>(
    catalog_mode: CatalogMode,
    save_catalog_name: Option<&str>,
    catalogs: &Catalogs,
    dep: &CatalogModeDep<'_>,
    prefix: &str,
) -> Result<CatalogDecision, CatalogVersionMismatchError> {
    // A `runtime:` specifier round-trips to `devEngines.runtime` through
    // the manifest writer; promoting it into a catalog would strand it in
    // `devDependencies`. Skip it, matching pnpm.
    if dep.bare_specifier.starts_with("runtime:") {
        return Ok(CatalogDecision::KeepDirect);
    }

    if catalog_mode == CatalogMode::Manual && save_catalog_name.is_none() {
        return Ok(CatalogDecision::KeepDirect);
    }

    let catalog_name = per_dep_catalog_name(dep.prev_specifier, save_catalog_name);
    let catalog_specifier = if catalog_name == DEFAULT_CATALOG_NAME {
        "catalog:".to_string()
    } else {
        format!("catalog:{catalog_name}")
    };

    if dep.bare_specifier == catalog_specifier {
        return Ok(CatalogDecision::Catalog {
            manifest_specifier: catalog_specifier,
            updated_entry: None,
        });
    }

    let wanted = WantedDependency {
        alias: dep.alias.to_string(),
        bare_specifier: catalog_specifier.clone(),
    };
    let entry = match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => found.resolution.specifier,
        _ => {
            return Ok(CatalogDecision::Catalog {
                manifest_specifier: catalog_specifier,
                updated_entry: Some(CatalogEntry {
                    catalog_name: catalog_name.to_string(),
                    specifier: dep.bare_specifier.to_string(),
                }),
            });
        }
    };

    if versions_equal(dep.bare_specifier, &entry) {
        return Ok(CatalogDecision::Catalog {
            manifest_specifier: catalog_specifier,
            updated_entry: None,
        });
    }

    match catalog_mode {
        CatalogMode::Strict => Err(CatalogVersionMismatchError {
            catalog_dep: format!("{}@{entry}", dep.alias),
            wanted_dep: format!("{}@{}", dep.alias, dep.bare_specifier),
        }),
        CatalogMode::Prefer => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Catalog version mismatch for \"{}\": using direct version \"{}\" instead of catalog version \"{entry}\".",
                    dep.alias, dep.bare_specifier,
                ),
                prefix: prefix.to_string(),
            }));
            Ok(CatalogDecision::KeepDirect)
        }
        CatalogMode::Manual => Ok(CatalogDecision::KeepDirect),
    }
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
/// specifier pins the named group; otherwise the global `--save-catalog-name`,
/// falling back to the default catalog. Mirrors pnpm's
/// [`getPerDepCatalogName`](https://github.com/pnpm/pnpm/blob/6c65cb5c18/installing/deps-installer/src/install/index.ts#L1223-L1234).
fn per_dep_catalog_name<'a>(
    prev_specifier: Option<&'a str>,
    save_catalog_name: Option<&'a str>,
) -> &'a str {
    prev_specifier
        .and_then(parse_catalog_protocol)
        .or(save_catalog_name)
        .unwrap_or(DEFAULT_CATALOG_NAME)
}

#[cfg(test)]
mod tests;
