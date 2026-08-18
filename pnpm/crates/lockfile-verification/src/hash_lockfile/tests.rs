use pnpm_lockfile::Lockfile;

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

#[test]
fn hash_is_stable_across_calls() {
    let lockfile = parse(LOCKFILE_YAML);
    let first = hash_lockfile(&lockfile);
    let second = hash_lockfile(&lockfile);
    assert_eq!(first, second);
    assert_eq!(first.len(), 64, "sha256 hex digest is 64 chars");
}

/// `HashMap` key iteration is non-deterministic; the normalize step
/// is what makes the hash stable.
#[test]
fn key_order_in_yaml_does_not_affect_hash() {
    let original = parse(LOCKFILE_YAML);
    let reordered = parse(LOCKFILE_YAML_REORDERED);
    assert_eq!(hash_lockfile(&original), hash_lockfile(&reordered));
}

/// Without this guarantee the cache could falsely short-circuit on a
/// drifted lockfile.
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

/// The digest has to describe the bytes the lockfile will be saved as,
/// not the in-memory map: a `time:` entry the writer prunes must not
/// move it, or a recorded verification would never be found again.
#[test]
fn a_pruned_time_entry_does_not_move_the_hash() {
    let pruned_to_nothing = parse(&with_time("scheduler@0.20.2: '2020-10-20T00:00:00.000Z'"));
    let empty = parse(&with_time("{}"));
    assert_eq!(hash_lockfile(&pruned_to_nothing), hash_lockfile(&empty));
}

#[test]
fn a_retained_time_entry_flips_the_hash() {
    let direct = parse(&with_time("react@17.0.2: '2021-03-22T14:00:00.000Z'"));
    let empty = parse(&with_time("{}"));
    assert_ne!(hash_lockfile(&direct), hash_lockfile(&empty));
}

/// `entry` is either a single `depPath: date` pair or the literal `{}`.
fn with_time(entry: &str) -> String {
    let separator = if entry == "{}" { " " } else { "\n  " };
    format!("{LOCKFILE_YAML}\ntime:{separator}{entry}\n")
}
