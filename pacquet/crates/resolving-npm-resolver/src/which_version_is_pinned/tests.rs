use super::which_version_is_pinned;
use pacquet_registry::PinnedVersion;

/// Table covering the pin classifications, including the parse-range
/// edge cases that derive the same results.
#[test]
fn matches_pnpm_which_version_is_pinned() {
    use PinnedVersion::{Major, Minor, None as NoneVariant, Patch};
    let cases: &[(&str, Option<PinnedVersion>)] = &[
        ("^1.0.0", Some(Major)),
        ("~1.0.0", Some(Minor)),
        ("1.0.0", Some(Patch)),
        ("*", Some(NoneVariant)),
        ("workspace:^1.0.0", Some(Major)),
        ("npm:foo@1.0.0", Some(Patch)),
        ("npm:@foo/foo@1.0.0", Some(Patch)),
        ("npm:foo@^1.0.0", Some(Major)),
        ("npm:@foo/foo@^1.0.0", Some(Major)),
        ("npm:@pnpm.e2e/qar@100.0.0", Some(Patch)),
        ("jsr:@foo/foo@1.0.0", Some(Patch)),
        ("jsr:foo@^1.0.0", Some(Major)),
        ("catalog:", None),
        ("catalog:default", None),
        ("catalog:foo", None),
        ("catalog:express4-21", None),
        ("~1.2.3", Some(Minor)),
        ("1.2", Some(Minor)),
        ("1", Some(Major)),
        ("1.x", Some(Minor)),
        ("1.2.x", Some(Patch)),
        ("1.2.0", Some(Patch)),
        ("0.0.0", Some(Patch)),
        ("^0", Some(Major)),
        ("^0.0.1", Some(Major)),
        ("~1", Some(Minor)),
        ("v1.2.3", Some(Patch)),
        ("1.2.3-alpha.1", Some(Patch)),
        (">=1.0.0", None),
        (">=1.0.0 <2.0.0", None),
        ("1.0.0 || 2.0.0", None),
        ("1.0.0 - 2.0.0", None),
        ("=1.2.3", None),
        ("~>1.2.3", None),
        ("workspace:~", None),
        ("x", None),
        ("latest", None),
        ("", None),
    ];
    for (spec, expected) in cases {
        assert_eq!(which_version_is_pinned(spec), *expected, "spec: {spec:?}");
    }
}
