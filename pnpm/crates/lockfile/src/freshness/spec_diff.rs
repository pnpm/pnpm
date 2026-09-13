use std::collections::BTreeMap;

/// Per-bucket diff against the manifest's flat union of deps.
/// Identical entries are omitted. Empty buckets render as nothing in
/// the `Display` impl so the resulting message lists only what the
/// user needs to fix.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpecDiff {
    pub added: BTreeMap<String, String>,
    pub removed: BTreeMap<String, String>,
    pub modified: BTreeMap<String, (String, String)>,
    /// The lockfile importer the diff belongs to, when the caller
    /// checked a specific importer. Rendered into the message so a
    /// workspace-wide freshness failure names the project whose
    /// manifest drifted instead of only the dependency.
    pub importer_id: Option<String>,
}

impl std::fmt::Display for SpecDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(importer_id) = &self.importer_id {
            write!(f, "\n* in importers[{importer_id:?}]:")?;
        }
        write_spec_bucket(f, "added", &self.added)?;
        write_spec_bucket(f, "removed", &self.removed)?;
        if !self.modified.is_empty() {
            let (dep, verb) = match self.modified.len() {
                1 => ("dependency", "is"),
                _ => ("dependencies", "are"),
            };
            write!(f, "\n* {} {dep} {verb} mismatched:", self.modified.len())?;
            for (key, (left, right)) in &self.modified {
                write!(f, "\n  - {key} (lockfile: {left}, manifest: {right})")?;
            }
        }
        Ok(())
    }
}

/// One `added` / `removed` bucket of [`SpecDiff`]'s `Display` impl.
///
/// Singular/plural matters here: the diff is rendered into
/// `ERR_PNPM_OUTDATED_LOCKFILE` CI output, which users see and may quote in
/// issues. "1 dependencies were added" reads wrong; the wording is pinned
/// per count.
fn write_spec_bucket(
    f: &mut std::fmt::Formatter<'_>,
    what: &str,
    specs: &BTreeMap<String, String>,
) -> std::fmt::Result {
    if specs.is_empty() {
        return Ok(());
    }
    let (dep, verb) = noun_verb_for(specs.len());
    write!(f, "\n* {} {dep} {verb} {what}: ", specs.len())?;
    let rendered: Vec<String> = specs
        .iter()
        .map(|(key, value)| format!("{key}@{value}"))
        .collect();
    write!(f, "{}", rendered.join(", "))
}

/// Singular/plural noun + past-tense verb for the `added` and
/// `removed` buckets in [`SpecDiff`]'s `Display` impl. Pulled out so
/// the arms stay readable.
fn noun_verb_for(n: usize) -> (&'static str, &'static str) {
    match n {
        1 => ("dependency", "was"),
        _ => ("dependencies", "were"),
    }
}

/// `true` when the flat-record diff is empty in all three buckets —
/// the manifest and the lockfile agree on the set of specifiers.
impl SpecDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}
