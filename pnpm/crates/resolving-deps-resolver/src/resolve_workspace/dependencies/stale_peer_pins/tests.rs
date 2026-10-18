use super::exact_version;
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
