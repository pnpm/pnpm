use super::{LockfileResolution, REVISION_SHA512};

#[test]
fn registry_revision_rejects_values_outside_the_positive_safe_integer_range() {
    for revision in ["0", "-1", "1.5", "9007199254740992", "'1'"] {
        let yaml = format!("integrity: {REVISION_SHA512}\nrevision: {revision}");
        let result = serde_saphyr::from_str::<LockfileResolution>(&yaml);
        assert!(result.is_err(), "revision {revision} must be rejected; got {result:?}");
    }
}
