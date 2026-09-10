use super::{assert_package_absent, assert_package_present, fresh_lockfile_only_with_overrides};

#[tokio::test]
async fn fresh_lockfile_resolves_catalog_protocol_in_overrides() {
    let (_dir, lockfile) = fresh_lockfile_only_with_overrides(
        &[("@pnpm.e2e/foo", "^100.0.0")],
        &[("@pnpm.e2e/foo@^100.0.0", "catalog:")],
        Some("catalog:\n  '@pnpm.e2e/foo': '100.0.0'\n"),
    )
    .await;

    assert_package_present(&lockfile, "@pnpm.e2e/foo@100.0.0");
    assert_package_absent(&lockfile, "@pnpm.e2e/foo@100.1.0");
    assert_eq!(
        lockfile
            .overrides
            .as_ref()
            .and_then(|overrides| overrides.get("@pnpm.e2e/foo@^100.0.0"))
            .map(String::as_str),
        Some("100.0.0"),
    );
}
