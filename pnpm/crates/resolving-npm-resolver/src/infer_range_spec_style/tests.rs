use super::infer_range_spec_style;
use pnpm_registry::RangeSpecStyle;

#[test]
fn matches_pnpm_infer_range_spec_style() {
    use RangeSpecStyle::{Exact, Major, Minor, None as NoneVariant, Patch};
    let cases: &[(&str, Option<RangeSpecStyle>)] = &[
        ("^1.0.0", Some(Major)),
        ("~1.0.0", Some(Minor)),
        ("1.0.0", Some(Patch)),
        ("=1.0.0", Some(Exact)),
        ("=1.0", Some(Minor)),
        ("=1", Some(Major)),
        ("*", Some(NoneVariant)),
        ("workspace:^1.0.0", Some(Major)),
        ("npm:foo@1.0.0", Some(Patch)),
        ("npm:@foo/foo@1.0.0", Some(Patch)),
        ("npm:foo@=1.0.0", Some(Exact)),
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
        ("=1.2.3", Some(Exact)),
        ("^=1.2.3", None),
        ("~=1.2.3", None),
        ("~>1.2.3", None),
        ("=1.2.3||", None),
        ("=1.2.3 -", None),
        ("1.2.3||", None),
        ("workspace:~", None),
        ("x", None),
        ("latest", None),
        ("", None),
    ];
    for (spec, expected) in cases {
        assert_eq!(infer_range_spec_style(spec), *expected, "spec: {spec:?}");
    }
}
