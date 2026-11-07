use super::{CanonicalPackageName, Identity, new_tombstone_from_hosted};
use serde_json::json;

#[test]
fn rewrite_tombstones_keep_identity_when_the_display_name_changes() {
    let publisher = Identity::account("publisher-id", "current-name");
    let package =
        CanonicalPackageName::parse("example-package", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({"maintainers": [
        {"name": "former-name", "userId": "publisher-id"},
        {"name": "current-name", "userId": "publisher-id"}
    ]});
    let tombstone = new_tombstone_from_hosted(&publisher, &package, &hosted);
    assert_eq!(tombstone.maintainers, vec!["publisher-id"]);
    assert_eq!(tombstone.unpublished_by.as_deref(), Some("publisher-id"));
}
