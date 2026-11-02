use super::Profile;
use pretty_assertions::assert_eq;

#[test]
fn accepts_two_path_segment_profiles() {
    for text in
        [Profile::NPM_INSTALL_MANIFEST, Profile::OCI_TAG, "npm-install-manifest/2", "a.b/1.0-rc"]
    {
        assert_eq!(
            text.parse::<Profile>()
                .unwrap()
                .as_str(),
            text,
        );
    }
}

#[test]
fn rejects_profiles_that_would_need_encoding() {
    for text in ["", "npm", "npm/", "/1", "npm/1/2", "Npm/1", "npm/1 ", "npm_x/1", "npm/%31"] {
        assert!(text.parse::<Profile>().is_err(), "accepted {text:?}");
    }
}

#[test]
fn knows_the_package_url_type_of_each_known_profile() {
    let purl_type = |text: &str| {
        text.parse::<Profile>()
            .unwrap()
            .purl_type()
    };
    assert_eq!(purl_type(Profile::NPM_INSTALL_MANIFEST), Some("npm"));
    assert_eq!(purl_type(Profile::CARGO_INDEX_ENTRY), Some("cargo"));
    assert_eq!(purl_type(Profile::PYTHON_CORE_METADATA), Some("pypi"));
    assert_eq!(purl_type(Profile::OCI_TAG), Some("oci"));
    assert_eq!(purl_type("npm-install-manifest/2"), None);
}

#[test]
fn deserializes_through_validation() {
    let profile: Profile = serde_json::from_str(r#""oci-tag/1""#).unwrap();
    assert!(profile.is_oci_tag());
    assert!(serde_json::from_str::<Profile>(r#""OCI""#).is_err());
    assert_eq!(serde_json::to_string(&profile).unwrap(), r#""oci-tag/1""#);
}
