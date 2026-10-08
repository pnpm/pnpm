use super::to_registry_url;

#[test]
fn unscoped_name_passes_through() {
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "lodash"),
        "https://registry.npmjs.org/lodash",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "acme-helper"),
        "https://registry.npmjs.org/acme-helper",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "acme_helper"),
        "https://registry.npmjs.org/acme_helper",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "acme.helper"),
        "https://registry.npmjs.org/acme.helper",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "acme~legacy"),
        "https://registry.npmjs.org/acme~legacy",
    );
}

#[test]
fn scoped_name_encodes_slash() {
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "@scope/pkg"),
        "https://registry.npmjs.org/@scope%2Fpkg",
    );
    assert_eq!(
        to_registry_url("https://registry.npmjs.org/", "@pnpm.e2e/hello-world"),
        "https://registry.npmjs.org/@pnpm.e2e%2Fhello-world",
    );
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
