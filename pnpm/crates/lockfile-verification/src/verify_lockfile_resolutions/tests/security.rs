use super::{
    SilentReporter, VerifyError, VerifyLockfileResolutionsOptions, parse,
    verify_lockfile_resolutions,
};

#[tokio::test]
async fn rejects_a_traversal_snapshot_package_name() {
    // The snapshot's own package name (the `snapshots:` key) becomes
    // `<slot>/node_modules/<name>` at extract time — a sink the alias
    // scan of the dependency maps alone doesn't cover.
    let lockfile = parse(
        "lockfileVersion: '9.0'

packages:

  ../../../escape@1.0.0:
    resolution: {integrity: sha512-deadbeef}

snapshots:

  ../../../escape@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("a traversal snapshot package name must be rejected");
    let VerifyError::InvalidDependencyAlias { breakdown, .. } = err else {
        panic!("expected InvalidDependencyAlias, got {err:?}");
    };
    assert!(breakdown.contains("../../../escape"), "breakdown {breakdown:?}");
}
