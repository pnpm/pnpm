use pnpm_lockfile::{
    Lockfile, LockfileResolution, extract_env_document, extract_main_document,
    normalize_lockfile_content,
};
use ssri::Integrity;

/// Whether the rewritten lockfile `next_text` is `previous` with only the
/// registry metadata of its packages replaced by that of `next`: the
/// `deprecated` notices, and an integrity recomputed with a stronger algorithm.
/// The registry can deprecate a version after it was locked, and
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
/// its registry or tarball resolution when only the algorithm got stronger, taken
/// from `source`, or `None` when `source` lacks one of its packages. Any other
/// difference in a resolution, such as a new revision or tarball URL, is left
/// for the comparison to reject.
fn with_registry_metadata_of(lockfile: &Lockfile, source: &Lockfile) -> Option<Lockfile> {
    let mut lockfile = lockfile.clone();
    if let Some(packages) = lockfile.packages.as_mut() {
        let source_packages = source.packages.as_ref()?;
        for (key, metadata) in packages {
            let source_metadata = source_packages.get(key)?;
            metadata.deprecated.clone_from(&source_metadata.deprecated);
            take_rehashed_resolution_integrity(
                &mut metadata.resolution,
                &source_metadata.resolution,
            );
        }
    }
    Some(lockfile)
}

/// Takes the integrity of `source` into `resolution` when both are registry
/// resolutions, or tarball resolutions that both record one, and `source` is
/// hashed with a stronger algorithm.
fn take_rehashed_resolution_integrity(
    resolution: &mut LockfileResolution,
    source: &LockfileResolution,
) {
    match (resolution, source) {
        (LockfileResolution::Registry(resolution), LockfileResolution::Registry(source)) => {
            take_rehashed_integrity(&mut resolution.integrity, &source.integrity);
        }
        (LockfileResolution::Tarball(resolution), LockfileResolution::Tarball(source)) => {
            if let (Some(integrity), Some(source)) =
                (resolution.integrity.as_mut(), source.integrity.as_ref())
            {
                take_rehashed_integrity(integrity, source);
            }
        }
        _ => {}
    }
}

/// Replaces `integrity` with `source` when `source` is hashed with a stronger
/// algorithm and shares none with `integrity`, so the same tarball was only
/// hashed differently. A different digest of the same algorithm means another
/// tarball, and a weaker algorithm means the registry stopped vouching for
/// the locked one, so `--check` must report both.
fn take_rehashed_integrity(integrity: &mut Integrity, source: &Integrity) {
    let shares_an_algorithm = integrity.hashes
        .iter()
        .any(|hash| {
            source.hashes
                .iter()
                .any(|source| hash.algorithm == source.algorithm)
        });
    if !shares_an_algorithm && is_stronger(source, integrity) {
        integrity.clone_from(source);
    }
}

/// Whether the strongest algorithm of `integrity` is stronger than every
/// algorithm of `than`. `ssri` orders `Algorithm` from strongest to weakest.
fn is_stronger(integrity: &Integrity, than: &Integrity) -> bool {
    let strongest = |integrity: &Integrity| {
        integrity.hashes
            .iter()
            .map(|hash| hash.algorithm)
            .min()
    };
    matches!(
        (strongest(integrity), strongest(than)),
        (Some(strongest), Some(than)) if strongest < than,
    )
}

#[cfg(test)]
mod tests;
