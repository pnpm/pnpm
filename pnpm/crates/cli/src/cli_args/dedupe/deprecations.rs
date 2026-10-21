use pnpm_lockfile::{
    Lockfile, extract_env_document, extract_main_document, normalize_lockfile_content,
};

/// Whether the rewritten lockfile `next_text` is `previous` with only the
/// `deprecated` notices of its packages replaced by those of `next`. The
/// registry can deprecate a version after it was locked, so a refreshed
/// notice is not a change for `--check` to fail on. Every other byte must
/// match, so a rewrite that also reformats the lockfile still fails.
pub(super) fn differs_only_in_deprecations(
    (previous_text, previous): (Option<&str>, Option<&Lockfile>),
    (next_text, next): (Option<&str>, Option<&Lockfile>),
) -> bool {
    let (Some(previous_text), Some(next_text), Some(previous), Some(next)) =
        (previous_text, next_text, previous, next)
    else {
        return false;
    };
    extract_env_document(previous_text) == extract_env_document(next_text)
        && is_serialized_as(previous, previous_text)
        && with_deprecations_of(previous, next)
            .is_some_and(|patched| is_serialized_as(&patched, next_text))
}

/// Whether `text` holds `lockfile` exactly as pnpm writes it, with no byte
/// order mark or CRLF line endings for the extraction to normalize away.
fn is_serialized_as(lockfile: &Lockfile, text: &str) -> bool {
    *normalize_lockfile_content(text) == *text
        && lockfile
            .to_yaml_string()
            .is_ok_and(|yaml| yaml == extract_main_document(text))
}

/// `lockfile` with each package's `deprecated` notice taken from `source`,
/// or `None` when `source` lacks one of its packages.
fn with_deprecations_of(lockfile: &Lockfile, source: &Lockfile) -> Option<Lockfile> {
    let mut lockfile = lockfile.clone();
    if let Some(packages) = lockfile.packages.as_mut() {
        let source_packages = source.packages.as_ref()?;
        for (key, metadata) in packages {
            metadata.deprecated.clone_from(&source_packages.get(key)?.deprecated);
        }
    }
    Some(lockfile)
}

#[cfg(test)]
mod tests;
