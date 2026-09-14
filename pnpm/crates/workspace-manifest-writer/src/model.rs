//! Parsed view of a `pnpm-workspace.yaml` used to decide catalog edits.
//!
//! Holds the original text verbatim (so untouched bytes survive) alongside
//! the decoded top-level key order and catalog data the edit pass consults.

use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashSet;

/// The `catalog:` / `catalogs:` slice of a `pnpm-workspace.yaml`, decoded
/// twice over the same source: once for the ordered top-level key list, once
/// for the catalog values.
#[derive(Default)]
pub(crate) struct Manifest {
    /// `configDependencies:` clean-specifier entries. Object-form
    /// entries (the legacy `{ tarball?, integrity }` shape) are dropped
    /// here — they're only consulted to detect a no-op write of an
    /// already-present clean specifier.
    pub(crate) config_dependencies: Option<IndexMap<String, String>>,
    /// `allowBuilds:` boolean entries. Consulted to detect a no-op write
    /// of an already-present value (and kept in sync as entries are
    /// upserted during a single `pnpm approve-builds` write).
    pub(crate) allow_builds: Option<IndexMap<String, AllowBuildValue>>,
    /// `patchedDependencies:` entries, keyed by `name[@version]`.
    pub(crate) patched_dependencies: Option<IndexMap<String, String>>,
    /// `overrides:` clean string entries, keyed by package selector.
    /// Consulted to detect a no-op write of an already-present clean
    /// specifier (the shape `pacquet link` and `pacquet audit --fix` write).
    pub(crate) overrides: Option<IndexMap<String, String>>,
    /// Override keys whose existing value is *not* a plain string (e.g. a
    /// nested mapping a user hand-wrote). The writer refuses to replace
    /// these with a scalar rather than corrupting the document.
    pub(crate) non_scalar_overrides: HashSet<String>,
    pub(crate) document: ManifestDocument,
    pub(crate) catalogs: CatalogEntries,
    pub(crate) exceptions: SecurityExceptions,
}

#[derive(Default)]
pub(crate) struct ManifestDocument {
    text: String,
    pub(crate) keys: Vec<String>,
    /// Whether the document separates its top-level blocks with blank lines,
    /// as judged by [`crate::edit::uses_blank_line_style`] on the original
    /// text. New blocks are inserted in the same style.
    pub(crate) blank_lines: bool,
}

#[derive(Default)]
pub(crate) struct CatalogEntries {
    /// `catalog:` shorthand for the default catalog.
    pub(crate) default: Option<IndexMap<String, String>>,
    /// `catalogs:` map of named catalogs (may include `default`).
    pub(crate) named: Option<IndexMap<String, IndexMap<String, String>>>,
}

#[derive(Default)]
pub(crate) struct SecurityExceptions {
    /// `auditConfig.ignoreGhsas:` list. Consulted to detect a no-op write
    /// of an already-present list.
    pub(crate) legacy_audit_ghsas: Option<Vec<String>>,
    /// `audit.ignore:` list — the canonical spelling, which wins over
    /// `auditConfig.ignoreGhsas` when both are present.
    pub(crate) audit: Option<Vec<String>>,
    /// `minimumReleaseAgeExclude:` list. Consulted to detect a no-op write
    /// of an already-present list.
    pub(crate) release_age: Option<Vec<String>>,
    /// `trustPolicyExclude:` list. Consulted to detect a no-op write
    /// of an already-present list.
    pub(crate) trust_policy: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror pnpm-workspace.yaml keys read by the manifest writer."
    )
)]
struct CatalogData {
    #[serde(default)]
    catalog: Option<IndexMap<String, String>>,
    #[serde(default)]
    catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
    #[serde(default, rename = "configDependencies")]
    config_dependencies: Option<IndexMap<String, ConfigDepValue>>,
    #[serde(default, rename = "allowBuilds")]
    allow_builds: Option<IndexMap<String, AllowBuildValue>>,
    #[serde(default, rename = "patchedDependencies")]
    patched_dependencies: Option<IndexMap<String, String>>,
    #[serde(default)]
    overrides: Option<IndexMap<String, OverrideValue>>,
    #[serde(default, rename = "auditConfig")]
    audit_config: Option<AuditConfigData>,
    #[serde(default)]
    audit: Option<AuditData>,
    #[serde(default, rename = "minimumReleaseAgeExclude")]
    minimum_release_age_exclude: Option<Vec<String>>,
    #[serde(default, rename = "trustPolicyExclude")]
    trust_policy_exclude: Option<Vec<String>>,
}

/// The `auditConfig` slice consulted for no-op detection.
#[derive(Default, Deserialize)]
struct AuditConfigData {
    #[serde(default, rename = "ignoreGhsas")]
    ignore_ghsas: Option<Vec<String>>,
}

/// The `audit` slice consulted for no-op detection and target selection.
#[derive(Default, Deserialize)]
struct AuditData {
    #[serde(default)]
    ignore: Option<Vec<String>>,
}

/// An `allowBuilds` value, tolerant of the string form pnpm also accepts
/// (a version spec) so decoding a manifest that uses it doesn't fail. Only
/// the boolean shape is retained — the only shape `pnpm approve-builds`
/// writes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum AllowBuildValue {
    Bool(bool),
    String(String),
    Other(serde::de::IgnoredAny),
}

/// A `configDependencies` value, tolerant of the legacy object form so
/// decoding a manifest that uses it doesn't fail. Only the clean-string
/// shape is retained.
#[derive(Deserialize)]
#[serde(untagged)]
enum ConfigDepValue {
    Clean(String),
    Other(serde::de::IgnoredAny),
}

/// An `overrides` value, tolerant of non-string forms (nested parent-scoped
/// objects) so decoding a manifest that uses them doesn't fail. Only the
/// string shape is retained; non-string keys are tracked separately so the
/// writer refuses to clobber them.
#[derive(Deserialize)]
#[serde(untagged)]
enum OverrideValue {
    String(String),
    Other(serde::de::IgnoredAny),
}

impl Manifest {
    /// Parse `original` (the file's contents, or `None` when the file is
    /// absent). An empty or whitespace/comment-only document decodes to an
    /// empty manifest, matching pnpm treating a nil parse as `{}`.
    pub(crate) fn parse(original: Option<&str>) -> Result<Self, Box<serde_saphyr::Error>> {
        let text = original.unwrap_or_default().to_string();

        if text.trim().is_empty() {
            return Ok(Manifest {
                document: crate::model::ManifestDocument { text, ..Default::default() },
                ..Manifest::default()
            });
        }

        let top: Option<IndexMap<String, serde::de::IgnoredAny>> =
            serde_saphyr::from_str(&text).map_err(Box::new)?;
        let top_level_keys: Vec<String> = top
            .map(|map| map.into_keys().collect())
            .unwrap_or_default();
        let blank_line_style = crate::edit::uses_blank_line_style(&text, &top_level_keys);

        let data: CatalogData = serde_saphyr::from_str(&text).map_err(Box::new)?;
        let (overrides, non_scalar_overrides) = split_overrides(data.overrides);

        Ok(Manifest {
            config_dependencies: data.config_dependencies.map(clean_config_dependencies),
            allow_builds: data.allow_builds.map(clean_allow_builds),
            patched_dependencies: data.patched_dependencies,
            overrides,
            non_scalar_overrides,
            document: crate::model::ManifestDocument {
                text,
                keys: top_level_keys,
                blank_lines: blank_line_style,
            },
            catalogs: crate::model::CatalogEntries { default: data.catalog, named: data.catalogs },
            exceptions: crate::model::SecurityExceptions {
                legacy_audit_ghsas: data.audit_config.and_then(|config| config.ignore_ghsas),
                audit: data.audit.and_then(|audit| audit.ignore),
                release_age: data.minimum_release_age_exclude,
                trust_policy: data.trust_policy_exclude,
            },
        })
    }
}

fn clean_config_dependencies(
    entries: IndexMap<String, ConfigDepValue>,
) -> IndexMap<String, String> {
    entries
        .into_iter()
        .filter_map(|(name, value)| match value {
            ConfigDepValue::Clean(specifier) => Some((name, specifier)),
            ConfigDepValue::Other(_) => None,
        })
        .collect()
}

fn clean_allow_builds(
    entries: IndexMap<String, AllowBuildValue>,
) -> IndexMap<String, AllowBuildValue> {
    entries
        .into_iter()
        .filter_map(|(name, value)| match value {
            AllowBuildValue::Bool(allowed) => Some((name, AllowBuildValue::Bool(allowed))),
            AllowBuildValue::String(s) => Some((name, AllowBuildValue::String(s))),
            AllowBuildValue::Other(_) => None,
        })
        .collect()
}

/// The clean string overrides, and the names of the ones written in a
/// non-scalar form.
fn split_overrides(
    entries: Option<IndexMap<String, OverrideValue>>,
) -> (Option<IndexMap<String, String>>, HashSet<String>) {
    let mut non_scalar_overrides = HashSet::new();
    let overrides = entries.map(|entries| {
        entries
            .into_iter()
            .filter_map(|(name, value)| match value {
                OverrideValue::String(specifier) => Some((name, specifier)),
                OverrideValue::Other(_) => {
                    non_scalar_overrides.insert(name);
                    None
                }
            })
            .collect()
    });
    (overrides, non_scalar_overrides)
}

impl ManifestDocument {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }
}
