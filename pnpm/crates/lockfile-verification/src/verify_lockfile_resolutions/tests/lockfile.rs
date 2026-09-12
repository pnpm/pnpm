use super::{
    Arc, CapturingVerifier, Mutex, ResolutionVerifier, SINGLE_PKG_LOCKFILE, SilentReporter,
    TWO_PKG_LOCKFILE, VerifyError, VerifyLockfileResolutionsOptions, parse,
    verify_lockfile_resolutions,
};

#[tokio::test]
async fn same_name_and_version_with_different_resolutions_are_both_verified() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

packages:

  react@17.0.2(peer@1.0.0):
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  react@17.0.2(peer@2.0.0):
    resolution: {integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}
",
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let verifier: Arc<dyn ResolutionVerifier> =
        Arc::new(CapturingVerifier { seen: Arc::clone(&seen), policy: serde_json::Map::new() });

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("both resolutions verify");

    let resolutions = seen.lock().expect("seen lock");
    assert_eq!(resolutions.len(), 2);
    assert_ne!(resolutions[0], resolutions[1]);
}

#[tokio::test]
async fn verifier_receives_the_lockfile_resolution_verbatim() {
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let expected = lockfile
        .packages
        .as_ref()
        .expect("packages")
        .values()
        .next()
        .expect("package")
        .resolution
        .clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let verifier: Arc<dyn ResolutionVerifier> =
        Arc::new(CapturingVerifier { seen: Arc::clone(&seen), policy: serde_json::Map::new() });

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("resolution verifies");

    assert_eq!(*seen.lock().expect("seen lock"), vec![expected]);
}

#[tokio::test]
async fn rejects_registry_style_key_backed_by_git_resolution_even_with_no_verifiers() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {type: git, repo: https://example.com/acme.git, commit: 0123456789abcdef0123456789abcdef01234567}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("registry-style key with a git resolution must be rejected");
    let VerifyError::ResolutionShapeMismatch { count, breakdown } = err else {
        panic!("expected ResolutionShapeMismatch, got {err:?}");
    };
    assert_eq!(count, 1);
    assert!(breakdown.contains("acme@1.0.0"), "breakdown: {breakdown}");
}

#[tokio::test]
async fn accepts_artifact_keys_with_non_registry_resolutions() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: github:org/acme
        version: git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567

packages:

  acme@git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567:
    resolution: {type: git, repo: https://example.com/acme.git, commit: 0123456789abcdef0123456789abcdef01234567}
    version: 1.0.0

snapshots:

  acme@git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567: {}
",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("artifact-keyed git entry passes the structural gate");
}

#[tokio::test]
async fn rejects_invalid_importer_alias_even_with_no_verifiers() {
    for alias in ["../../../escape", "@scope/../../escape", ".bin", ".pnpm", "node_modules"] {
        let yaml = format!(
            "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      '{alias}':\n        specifier: ^1.0.0\n        version: 1.0.0\n\npackages:\n\n  real@1.0.0:\n    resolution: {{integrity: sha512-deadbeef}}\n\nsnapshots:\n\n  real@1.0.0: {{}}\n",
        );
        let lockfile = parse(&yaml);
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .expect_err("invalid importer alias must be rejected");
        let VerifyError::InvalidDependencyAlias { count, breakdown } = err else {
            panic!("expected InvalidDependencyAlias for {alias:?}, got {err:?}");
        };
        assert_eq!(count, 1);
        assert!(breakdown.contains(alias), "breakdown {breakdown:?} should mention {alias:?}");
    }
}

#[test]
fn verify_lockfile_dependency_names_rejects_a_packages_less_lockfile() {
    // A lockfile with only `link:` deps has no `packages:` section, so
    // the resolution fan-out short-circuits — but the offline name check
    // must still reject a traversal importer alias.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      '../../escaped-link':
        specifier: link:./local
        version: link:local
",
    );
    assert!(lockfile.packages.is_none(), "fixture must have no packages section");
    let err = super::super::verify_lockfile_dependency_names(&lockfile)
        .expect_err("a traversal alias in a packages-less lockfile must be rejected");
    assert!(matches!(err, VerifyError::InvalidDependencyAlias { .. }), "got {err:?}");
}

#[test]
fn verify_lockfile_dependency_names_accepts_a_clean_lockfile() {
    super::super::verify_lockfile_dependency_names(&parse(TWO_PKG_LOCKFILE))
        .expect("a lockfile with valid names must pass");
}
