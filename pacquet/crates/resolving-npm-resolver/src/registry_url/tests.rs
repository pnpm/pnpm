use super::{encode_pkg_name_path, to_registry_url};

/// Unscoped names go through unchanged — every character npm allows
/// (`a-z 0-9 - _ . ~`) is in `encodeURIComponent`'s unreserved set.
#[test]
fn unscoped_name_passes_through() {
    assert_eq!(encode_pkg_name_path("lodash"), "lodash");
    assert_eq!(encode_pkg_name_path("acme-helper"), "acme-helper");
    assert_eq!(encode_pkg_name_path("acme_helper"), "acme_helper");
    assert_eq!(encode_pkg_name_path("acme.helper"), "acme.helper");
    assert_eq!(encode_pkg_name_path("acme~legacy"), "acme~legacy");
}

/// Scoped name: the leading `@` is preserved, the `/` after the
/// scope is percent-encoded. Mirrors upstream's
/// `@${encodeURIComponent(pkgName.slice(1))}`.
#[test]
fn scoped_name_encodes_slash() {
    assert_eq!(encode_pkg_name_path("@scope/pkg"), "@scope%2Fpkg");
    assert_eq!(encode_pkg_name_path("@pnpm.e2e/hello-world"), "@pnpm.e2e%2Fhello-world");
}

/// `to_registry_url` joins the registry with a slash and appends
/// the encoded name. The registry is normalised to trailing slash
/// before joining so an unscoped vs trailing-slash registry config
/// produces the same URL.
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
