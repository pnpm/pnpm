use super::{encode_pkg_name_path, to_registry_url};

#[test]
fn unscoped_name_passes_through() {
    assert_eq!(encode_pkg_name_path("lodash"), "lodash");
    assert_eq!(encode_pkg_name_path("acme-helper"), "acme-helper");
    assert_eq!(encode_pkg_name_path("acme_helper"), "acme_helper");
    assert_eq!(encode_pkg_name_path("acme.helper"), "acme.helper");
    assert_eq!(encode_pkg_name_path("acme~legacy"), "acme~legacy");
}

#[test]
fn scoped_name_encodes_slash() {
    assert_eq!(encode_pkg_name_path("@scope/pkg"), "@scope%2Fpkg");
    assert_eq!(encode_pkg_name_path("@pnpm.e2e/hello-world"), "@pnpm.e2e%2Fhello-world");
}

#[test]
fn url_join_normalizes_trailing_slash() {
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "@scope/pkg"),
        "https://registry.npmjs.org/@scope%2Fpkg",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org", "@scope/pkg"),
        "https://registry.npmjs.org/@scope%2Fpkg",
    );
}
