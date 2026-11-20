use super::{
    BTreeMap,
    CancellableProvider,
    InnerProvider,
    PackageKey,
    Ranges,
    Version,
    compatibility_line,
};

#[test]
fn tries_each_preferred_version_when_earlier_line_is_out_of_range() {
    let package = PackageKey::Requirement {
        name: "foo".to_string(),
        requirement: "*".to_string(),
        default_features: true,
        features: Vec::new(),
    };
    let mut inner = InnerProvider::new();
    for version in [Version::new(1, 0, 0), Version::new(2, 0, 0), Version::new(3, 0, 0)] {
        let line = PackageKey::Registry {
            name: "foo".to_string(),
            compatibility: compatibility_line(&version),
        };
        let admitted = Ranges::singleton(version.clone());
        inner.add_dependencies(package.clone(), version, [(line, admitted)]);
    }
    let preferred_versions =
        BTreeMap::from([("foo".to_string(), vec![Version::new(2, 0, 0), Version::new(1, 0, 0)])]);
    let should_cancel = || false;
    let provider = CancellableProvider {
        inner,
        preferred_versions: &preferred_versions,
        should_cancel: &should_cancel,
    };
    let range =
        Ranges::singleton(Version::new(1, 0, 0)).union(&Ranges::singleton(Version::new(3, 0, 0)));

    assert_eq!(provider.preferred_version(&package, &range), Some(Version::new(1, 0, 0)));
}
