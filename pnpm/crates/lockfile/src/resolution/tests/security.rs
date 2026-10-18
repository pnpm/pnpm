use super::{LockfileResolution, REVISION_SHA512};
use crate::{RegistryResolution, TarballRevision};

#[test]
fn registry_revision_rejects_values_outside_the_safe_integer_range() {
    for revision in ["-1", "1.5", "9007199254740992", "'1'"] {
        let yaml = format!("integrity: {REVISION_SHA512}\nrevision: {revision}");
        let result = serde_saphyr::from_str::<LockfileResolution>(&yaml);
        assert!(result.is_err(), "revision {revision} must be rejected; got {result:?}");
    }
}

#[test]
fn registry_revision_zero_is_recorded_explicitly() {
    let yaml = format!("integrity: {REVISION_SHA512}\nrevision: 0");
    let resolution = serde_saphyr::from_str::<LockfileResolution>(&yaml).unwrap();
    let LockfileResolution::Registry(RegistryResolution { revision, .. }) = &resolution else {
        panic!("expected a registry resolution, got {resolution:?}");
    };
    assert_eq!(*revision, Some(TarballRevision::try_from(0).unwrap()));
    let written = serde_saphyr::to_string(&resolution).unwrap();
    assert!(written.contains("revision: 0"), "{written}");
}
