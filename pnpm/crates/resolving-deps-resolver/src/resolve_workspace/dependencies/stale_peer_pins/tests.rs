use super::{exact_version, locked_importer_version};
use pretty_assertions::assert_eq;

#[test]
fn exact_version_canonicalizes_prefixed_specifiers() {
    for spec in ["1.0.0", "v1.0.0", "=1.0.0", " =v1.0.0 "] {
        assert_eq!(
            exact_version(spec)
                .map(|version| version.to_string())
                .ok()
                .as_deref(),
            Some("1.0.0"),
            "{spec}",
        );
    }
}

#[test]
fn exact_version_rejects_ranges() {
    for spec in ["^1.0.0", "1.0", ">=1.0.0"] {
        assert!(exact_version(spec).is_err(), "{spec}");
    }
}

/// An importer dependency locked under a registry-qualified key pins the
/// version the key carries.
#[test]
fn locked_importer_version_reads_a_registry_qualified_version() {
    let lockfile: pnpm_lockfile::Lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "react": { "specifier": "^17.0.0", "version": "work:17.0.2" },
                },
            },
        },
    }))
    .expect("parse lockfile");
    assert_eq!(
        locked_importer_version(&lockfile, ".", "react").map(|version| version.to_string()),
        Some("17.0.2".to_string()),
    );
}
