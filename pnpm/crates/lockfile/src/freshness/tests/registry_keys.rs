use super::{Lockfile, StalenessReason, text_block};
use crate::{DeclaredRegistryPrefixes, RegistryContext, check_lockfile_registry_keys};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, HashMap};

const INTEGRITY: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";

/// `@work/foo` is keyed plainly, `bar` by the `work` prefix.
fn lockfile() -> Lockfile {
    serde_saphyr::from_str(&format!(
        text_block! {
            "lockfileVersion: '9.0'"
            "importers:"
            "  .:"
            "    dependencies:"
            "      '@work/foo':"
            "        specifier: ^1.0.0"
            "        version: 1.0.0"
            "      bar:"
            "        specifier: work:1.0.0"
            "        version: work:1.0.0"
            "packages:"
            "  '@work/foo@1.0.0':"
            "    resolution: {{integrity: {INTEGRITY}}}"
            "  'bar@work:1.0.0':"
            "    resolution: {{integrity: {INTEGRITY}}}"
            "snapshots:"
            "  '@work/foo@1.0.0': {{}}"
            "  'bar@work:1.0.0': {{}}"
        },
        INTEGRITY = INTEGRITY,
    ))
    .expect("parse lockfile")
}

/// The `@work` scope routes to the registry the declared prefixes name.
fn context(declared: &[(&str, &str)]) -> RegistryContext {
    RegistryContext {
        registries: HashMap::from([
            ("default".to_string(), "https://registry.npmjs.org/".to_string()),
            ("@work".to_string(), "https://npm.example.com/".to_string()),
        ]),
        registries_by_prefix: declared
            .iter()
            .map(|(prefix, registry)| ((*prefix).to_string(), (*registry).to_string()))
            .collect(),
        declared_prefixes: DeclaredRegistryPrefixes::new(declared.iter().copied()),
        registry_options_by_url: BTreeMap::default(),
    }
}

#[test]
fn keys_are_current_when_no_prefix_is_declared() {
    assert_eq!(check_lockfile_registry_keys(&lockfile(), &context(&[])), Ok(()));
}

#[test]
fn a_plain_key_of_a_registry_with_a_declared_prefix_is_outdated() {
    assert_eq!(
        check_lockfile_registry_keys(
            &lockfile(),
            &context(&[("work", "https://npm.example.com/")]),
        ),
        Err(StalenessReason::OutdatedRegistryKey {
            key: "@work/foo@1.0.0".to_string(),
            current: "@work/foo@work:1.0.0".to_string(),
        }),
    );
}

#[test]
fn the_smallest_outdated_key_is_reported() {
    // `alpha` also names the registry, so `bar@work:1.0.0` is outdated too;
    // `@work/foo@1.0.0` sorts first.
    let context =
        context(&[("work", "https://npm.example.com/"), ("alpha", "https://npm.example.com/")]);
    assert_eq!(
        check_lockfile_registry_keys(&lockfile(), &context),
        Err(StalenessReason::OutdatedRegistryKey {
            key: "@work/foo@1.0.0".to_string(),
            current: "@work/foo@alpha:1.0.0".to_string(),
        }),
    );
}
