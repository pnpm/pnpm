use pacquet_lockfile::Lockfile;

use super::hash_lockfile;

const LOCKFILE_YAML: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      lodash:
        specifier: ^4.17.21
        version: 4.17.21
      react:
        specifier: ^17.0.2
        version: 17.0.2
";

const LOCKFILE_YAML_REORDERED: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      react:
        specifier: ^17.0.2
        version: 17.0.2
      lodash:
        specifier: ^4.17.21
        version: 4.17.21
";

fn parse(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse fixture lockfile")
}

/// The same in-memory `Lockfile` hashes the same regardless of how
/// many times we ask. This is the floor of the cache contract — a
/// successive run on the same lockfile must produce the same key.
#[test]
fn hash_is_stable_across_calls() {
    let lockfile = parse(LOCKFILE_YAML);
    let first = hash_lockfile(&lockfile);
    let second = hash_lockfile(&lockfile);
    assert_eq!(first, second);
    assert_eq!(first.len(), 64, "sha256 hex digest is 64 chars");
}

/// Lockfiles that parse to the same logical content but were
/// written with different YAML key orders produce the same hash.
/// `HashMap` key iteration is non-deterministic; the normalize step
/// is what makes the hash stable.
#[test]
fn key_order_in_yaml_does_not_affect_hash() {
    let original = parse(LOCKFILE_YAML);
    let reordered = parse(LOCKFILE_YAML_REORDERED);
    assert_eq!(hash_lockfile(&original), hash_lockfile(&reordered));
}

/// A meaningful change to the lockfile (a new dependency entry)
/// flips the hash. Without this guarantee the cache could falsely
/// short-circuit on a drifted lockfile.
#[test]
fn semantic_changes_flip_the_hash() {
    let original = parse(LOCKFILE_YAML);
    let drifted = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      lodash:
        specifier: ^4.17.21
        version: 4.17.21
      react:
        specifier: ^17.0.2
        version: 17.0.2
      vite:
        specifier: ^5.0.0
        version: 5.0.0
",
    );
    assert_ne!(hash_lockfile(&original), hash_lockfile(&drifted));
}
