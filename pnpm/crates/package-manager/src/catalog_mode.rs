//! `catalogMode` reconciliation for `pacquet add` / `pacquet update`.
//!
//! Two halves of catalog-mode handling:
//!
//! - the **gate**: a direct version disagreeing with a matching
//!   `catalog:` entry is rejected ([`CatalogMode::Strict`]) or kept
//!   with a warning ([`CatalogMode::Prefer`]);
//! - the **auto-cataloging** decision (`saveCatalogName` /
//!   `catalogLookup`): a matching or not-yet-cataloged dependency is
//!   rewritten to `catalog:` / `catalog:<name>` and, when no entry
//!   exists yet, recorded for write-back to `pnpm-workspace.yaml`.

use crate::is_workspace_local_path_specifier;
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_resolver::{CatalogResolutionResult, WantedDependency, resolve_from_catalog};
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pnpm_config::CatalogMode;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_local_resolver::is_local_filesystem_specifier;

/// Wanted dependency outside the version range defined in catalog.
///
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

pub(crate) struct CatalogDecisionOutcome {
    pub decision: CatalogDecision,
    pub warning: Option<LogEvent>,
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
    let outcome = decide_catalog_outcome(catalog_mode, save_catalog_name, catalogs, dep, prefix)?;
    if let Some(warning) = outcome.warning {
        Reporter::emit(&warning);
    }
    Ok(outcome.decision)
}

/// Decide without emitting so concurrent callers can replay warnings in a
/// deterministic order.
pub(crate) fn decide_catalog_outcome(
    catalog_mode: CatalogMode,
    save_catalog_name: Option<&str>,
    catalogs: &Catalogs,
    dep: &CatalogModeDep<'_>,
    prefix: &str,
) -> Result<CatalogDecisionOutcome, CatalogVersionMismatchError> {
    // A `runtime:` specifier round-trips to `devEngines.runtime` through
    // the manifest writer; promoting it into a catalog would strand it in
    // `devDependencies`. Skip it, matching pnpm.
    if dep.bare_specifier.starts_with("runtime:") {
        return Ok(CatalogDecisionOutcome { decision: CatalogDecision::KeepDirect, warning: None });
    }

    if is_project_relative_path(dep.bare_specifier) {
        return Ok(CatalogDecisionOutcome { decision: CatalogDecision::KeepDirect, warning: None });
    }

    if catalog_mode == CatalogMode::Manual && save_catalog_name.is_none() {
        return Ok(CatalogDecisionOutcome { decision: CatalogDecision::KeepDirect, warning: None });
    }

    let catalog_name = per_dep_catalog_name(dep.prev_specifier, save_catalog_name);
    let catalog_specifier = if catalog_name == DEFAULT_CATALOG_NAME {
        "catalog:".to_string()
    } else {
        format!("catalog:{catalog_name}")
    };

    if dep.bare_specifier == catalog_specifier {
        return Ok(CatalogDecisionOutcome {
            decision: CatalogDecision::Catalog {
                manifest_specifier: catalog_specifier,
                updated_entry: None,
            },
            warning: None,
        });
    }

    decide_catalog_entry(catalog_mode, catalogs, dep, prefix, catalog_name, catalog_specifier)
}

fn decide_catalog_entry(
    catalog_mode: CatalogMode,
    catalogs: &Catalogs,
    dep: &CatalogModeDep<'_>,
    prefix: &str,
    catalog_name: &str,
    catalog_specifier: String,
) -> Result<CatalogDecisionOutcome, CatalogVersionMismatchError> {
    let wanted = WantedDependency {
        alias: dep.alias.to_string(),
        bare_specifier: catalog_specifier.clone(),
    };
    let entry = match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => found.resolution.specifier,
        _ => {
            return Ok(CatalogDecisionOutcome {
                decision: CatalogDecision::Catalog {
                    manifest_specifier: catalog_specifier,
                    updated_entry: Some(CatalogEntry {
                        catalog_name: catalog_name.to_string(),
                        specifier: dep.bare_specifier.to_string(),
                    }),
                },
                warning: None,
            });
        }
    };

    if catalog_covers(&entry, dep.bare_specifier) {
        return Ok(CatalogDecisionOutcome {
            decision: CatalogDecision::Catalog {
                manifest_specifier: catalog_specifier,
                updated_entry: None,
            },
            warning: None,
        });
    }

    catalog_mismatch(catalog_mode, dep, prefix, &entry)
}

fn catalog_mismatch(
    catalog_mode: CatalogMode,
    dep: &CatalogModeDep<'_>,
    prefix: &str,
    entry: &str,
) -> Result<CatalogDecisionOutcome, CatalogVersionMismatchError> {
    match catalog_mode {
        CatalogMode::Strict => Err(CatalogVersionMismatchError {
            catalog_dep: format!("{}@{entry}", dep.alias),
            wanted_dep: format!("{}@{}", dep.alias, dep.bare_specifier),
        }),
        CatalogMode::Prefer => Ok(CatalogDecisionOutcome {
            decision: CatalogDecision::KeepDirect,
            warning: Some(LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Catalog version mismatch for "{}": using direct version "{}" instead of catalog version "{entry}"."#,
                    dep.alias, dep.bare_specifier,
                ),
                prefix: prefix.to_string(),
            })),
        }),
        CatalogMode::Manual => {
            Ok(CatalogDecisionOutcome { decision: CatalogDecision::KeepDirect, warning: None })
        }
    }
}

/// Whether `specifier` names a path resolved against the project that
/// declares it — a `file:` / `link:` protocol, a bare path or tarball
/// filename, or a `workspace:` pointing at a directory rather than a range.
///
/// A catalog entry is read by every project that references it, so it
/// cannot mean the same directory for all of them. The catalog resolver
/// already refuses a `link:` / `file:` entry outright
/// (`ERR_PNPM_CATALOG_ENTRY_INVALID_SPEC`); it accepts a `workspace:` one,
/// which is worse — every consumer silently resolves the relative path from
/// its own directory. Auto-cataloging leaves all of them alone.
fn is_project_relative_path(specifier: &str) -> bool {
    is_local_filesystem_specifier(specifier) || is_workspace_local_path_specifier(specifier)
}

/// Whether the catalog entry already covers the wanted specifier, so the
/// dependency can keep resolving through the catalog: the entry is a range
/// that holds the wanted version, or that holds every version the wanted
/// range allows.
///
/// Coverage runs in that direction because the catalog — not the dependency —
/// decides which version a `catalog:` reference resolves to. A wanted range
/// wider than the entry is not covered: swapping it for `catalog:` would
/// narrow what the dependency accepts.
pub(crate) fn catalog_covers(entry: &str, wanted: &str) -> bool {
    let Ok(entry_range) = Range::parse(entry) else {
        return false;
    };
    if let Ok(wanted_version) = Version::parse(wanted) {
        return entry_range.satisfies(&wanted_version);
    }
    // `Range::allows_all` only asks whether *some* alternative of the entry
    // holds *some* alternative of the wanted range, so a union such as
    // `^1 || ^3` would pass on its first branch alone. Ask per alternative.
    wanted.split("||").all(|alternative| entry_covers_alternative(&entry_range, alternative))
}

/// Whether `entry` holds every version the range alternative `wanted` allows.
///
/// [`Range::allows_all`] compares endpoints, which misses npm's rule that a
/// prerelease is eligible only for a comparator carrying a prerelease of the
/// same `major.minor.patch`: by endpoints alone `^1.0.0` looks wide enough for
/// `^1.2.0-beta.1`, though it admits no `1.2.0` prerelease. The prereleases an
/// alternative can admit are the ones its own comparators name, so ask
/// [`Range::satisfies`] about each of those.
fn entry_covers_alternative(entry: &Range, wanted: &str) -> bool {
    let Ok(wanted_range) = Range::parse(wanted) else {
        return false;
    };
    if !entry.allows_all(&wanted_range) {
        return false;
    }
    wanted
        .split_whitespace()
        .filter_map(|token| {
            Version::parse(token.trim_start_matches(['>', '<', '=', '^', '~', 'v'])).ok()
        })
        .filter(Version::is_prerelease)
        .all(|named| entry.satisfies(&named))
}

/// The catalog group a dependency belongs to: a previous `catalog:<name>`
/// specifier pins the named group; otherwise the global `--save-catalog-name`,
/// falling back to the default catalog.
pub(crate) fn per_dep_catalog_name<'a>(
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
