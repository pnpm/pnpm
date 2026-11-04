use pnpm_lockfile::{
    Lockfile, LockfileResolution, extract_env_document, extract_main_document,
    normalize_lockfile_content,
};
use ssri::Integrity;

/// Whether the rewritten lockfile `next_text` is `previous` with only the
/// registry metadata of its packages replaced by that of `next`: the
/// `deprecated` notices, and a registry integrity recomputed with another
/// algorithm. The registry can deprecate a version after it was locked, and
/// a registry proxy can start reporting a `sha512` integrity for a version
/// it only reported the `sha1` shasum of, so neither is a change for
/// `--check` to fail on. Every other byte must match, so a rewrite that also
/// reformats the lockfile still fails.
pub(super) fn differs_only_in_registry_metadata(
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
        && with_registry_metadata_of(previous, next)
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

/// `lockfile` with each package's `deprecated` notice, and the integrity of
/// its registry or tarball resolution when only the algorithm changed for the
/// same artifact, taken from `source`, or `None` when `source` lacks one of
/// its packages.
fn with_registry_metadata_of(lockfile: &Lockfile, source: &Lockfile) -> Option<Lockfile> {
    let mut lockfile = lockfile.clone();
    if let Some(packages) = lockfile.packages.as_mut() {
        let source_packages = source.packages.as_ref()?;
        for (key, metadata) in packages {
            let source_metadata = source_packages.get(key)?;
            metadata.deprecated.clone_from(&source_metadata.deprecated);
            match (&mut metadata.resolution, &source_metadata.resolution) {
                (
                    LockfileResolution::Registry(resolution),
                    LockfileResolution::Registry(source),
                ) if resolution.revision == source.revision
                    && !shares_an_algorithm(&resolution.integrity, &source.integrity) =>
                {
                    resolution.integrity.clone_from(&source.integrity);
                }
                (LockfileResolution::Tarball(resolution), LockfileResolution::Tarball(source))
                    if resolution.tarball == source.tarball
                        && resolution.revision == source.revision
                        && resolution.integrity
                            .as_ref()
                            .zip(source.integrity.as_ref())
                            .is_some_and(|(a, b)| !shares_an_algorithm(a, b)) =>
                {
                    resolution.integrity.clone_from(&source.integrity);
                }
                _ => {}
            }
        }
    }
    Some(lockfile)
}

/// Whether `a` and `b` hold a digest of the same algorithm. When they do and
/// still differ, the registry serves another tarball, which `--check` must
/// report; when they don't, the same tarball was merely hashed differently.
fn shares_an_algorithm(a: &Integrity, b: &Integrity) -> bool {
    a.hashes
        .iter()
        .any(|a| {
            b.hashes
                .iter()
                .any(|b| a.algorithm == b.algorithm)
        })
}

#[cfg(test)]
mod tests;
