use pnpm_lockfile::{Lockfile, extract_env_document};
use std::collections::HashMap;

/// Whether two lockfile snapshots, each given as its text and its parsed
/// main document, differ only in the `deprecated` notices of their packages.
/// The registry can deprecate a version after it was locked, so a refreshed
/// notice is not a change for `--check` to fail on.
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
        && without_deprecations(previous) == without_deprecations(next)
}

fn without_deprecations(lockfile: &Lockfile) -> Lockfile {
    let mut lockfile = lockfile.clone();
    for metadata in lockfile.packages.iter_mut().flat_map(HashMap::values_mut) {
        metadata.deprecated = None;
    }
    lockfile
}
