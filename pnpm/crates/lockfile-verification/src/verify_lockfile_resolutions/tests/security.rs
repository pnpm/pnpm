use crate::{
    LocalResolutionPolicy,
    verify_lockfile_local_resolutions,
};

use super::{
    Arc,
    CapturingVerifier,
    FetchFails,
    Mutex,
    RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    ResolutionVerifier,
    SilentReporter,
    TempDir,
    VerifyError,
    VerifyLockfileResolutionsOptions,
    collect_resolution_policy_violations,
    lockfile_verification_is_cached,
    lockfile_verification_is_cached_by_content,
    parse,
    verify_lockfile_resolutions,
};

#[tokio::test]
async fn rejects_a_traversal_snapshot_package_name() {
    // The snapshot's own package name (the `snapshots:` key) becomes
    // `<slot>/node_modules/<name>` at extract time, a sink the alias
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

#[test]
fn rejects_a_traversal_package_key_when_snapshots_absent() {
    for raw_key in ["../../../escape@1.0.0", r"..\escape@1.0.0"] {
        let yaml = format!(
            "lockfileVersion: '9.0'\n\npackages:\n\n  '{raw_key}':\n    resolution: {{integrity: sha512-deadbeef}}\n",
        );
        let lockfile = parse(&yaml);
        let err = crate::verify_lockfile_dependency_names(&lockfile)
            .expect_err("a traversal packages key must be rejected even without snapshots");
        let VerifyError::InvalidDependencyAlias { breakdown, .. } = err else {
            panic!("expected InvalidDependencyAlias for {raw_key}, got {err:?}");
        };
        let expected =
            serde_json::to_string(raw_key.trim_end_matches("@1.0.0")).expect("serialize alias");
        assert!(breakdown.contains(&expected), "breakdown {breakdown:?}");
    }
}

async fn assert_shape_violation_for_resolution(label: &str, resolution: &str) {
    let yaml = format!(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      safe-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  safe-pkg@1.0.0:
    resolution: {resolution}
snapshots:
  safe-pkg@1.0.0: {{}}
",
    );
    let lockfile = parse(&yaml);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capturing: Arc<dyn ResolutionVerifier> =
        Arc::new(CapturingVerifier { seen: Arc::clone(&seen), policy: serde_json::Map::new() });
    let fetch_fails =
        FetchFails::new("upstream must not be contacted") as Arc<dyn ResolutionVerifier>;

    for verifiers in [&[][..], &[capturing][..], &[fetch_fails][..]] {
        let violations = collect_resolution_policy_violations(&lockfile, verifiers, None)
            .await
            .expect("shape mismatch returns violations instead of transport error");
        assert_eq!(violations.len(), 1, "{label}: {violations:?}");
        assert_eq!(violations[0].name.to_string(), "safe-pkg", "{label}");
        assert_eq!(violations[0].version, "1.0.0", "{label}");
        assert_eq!(violations[0].code, RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE, "{label}");
    }
    assert!(
        seen.lock()
            .expect("seen lock")
            .is_empty(),
        "{label} must short-circuit before running registry verifiers",
    );
}

#[tokio::test]
async fn collect_resolution_policy_violations_reports_shape_mismatches() {
    let cases = [
        ("file-tarball", "{integrity: sha512-deadbeef, tarball: 'file:alt.tgz'}"),
        ("directory", "{type: directory, directory: '../alt'}"),
        (
            "git",
            "{type: git, repo: 'https://example.com/safe-pkg.git', commit: 0123456789abcdef0123456789abcdef01234567}",
        ),
    ];
    for (label, resolution) in cases {
        assert_shape_violation_for_resolution(label, resolution).await;
    }
}

fn make_policy_verifier(policy: serde_json::Value) -> Arc<dyn ResolutionVerifier> {
    let serde_json::Value::Object(map) = policy else {
        panic!("expected policy object, got {policy}");
    };
    Arc::new(CapturingVerifier { seen: Arc::new(Mutex::new(Vec::new())), policy: map })
}

#[tokio::test]
async fn stale_or_boolean_shape_flag_cache_record_cannot_bypass_resolution_shape_mismatch() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      safe-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  safe-pkg@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'file:alt.tgz'}
snapshots:
  safe-pkg@1.0.0: {}
";
    let lockfile = parse(yaml);
    let err = crate::VerifiedOfflineLockfile::verify(&lockfile)
        .expect_err("offline lockfile proof must reject shape mismatch");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");

    let active_verifier = make_policy_verifier(serde_json::json!({ "mra": 60 }));
    let seeded_policies = [
        serde_json::json!({ "mra": 60 }),
        serde_json::json!({
            "mra": 60,
            "resolutionShapeCheck": true,
            "dependencyAliasCheck": true,
            "variationResolutionCheck": true,
        }),
    ];

    for seeded_policy in seeded_policies {
        let dir = TempDir::new().expect("tempdir");
        let lockfile_path = dir.path().join("pnpm-lock.yaml");
        std::fs::write(&lockfile_path, yaml).expect("write lockfile");
        let cache_dir = dir.path().join("cache");
        let seeded_verifier = make_policy_verifier(seeded_policy.clone());
        crate::record_verification(
            &cache_dir,
            &lockfile_path,
            &[seeded_verifier],
            || crate::hash_lockfile(&lockfile),
            crate::CachePrecomputed::default(),
        );

        let verifiers = std::slice::from_ref(&active_verifier);
        assert!(!lockfile_verification_is_cached(
            &cache_dir,
            &lockfile_path,
            &lockfile,
            verifiers,
            &crate::ExoticSubdepsPolicy::Allow,
        ));
        assert!(!lockfile_verification_is_cached_by_content(
            &cache_dir,
            &lockfile,
            verifiers,
            &crate::ExoticSubdepsPolicy::Allow,
        ));

        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            verifiers,
            &VerifyLockfileResolutionsOptions {
                lockfile_path: Some(&lockfile_path),
                cache_dir: Some(&cache_dir),
                ..Default::default()
            },
        )
        .await
        .expect_err("cached entry without resolutionShapeCheck: 2 must not bypass shape check");
        assert!(
            matches!(err, VerifyError::ResolutionShapeMismatch { .. }),
            "seeded={seeded_policy}, got {err:?}",
        );
    }
}

#[tokio::test]
async fn rejects_non_file_package_keys_backed_by_local_resolutions() {
    for (pkg_key, resolution) in [
        ("remote-pkg@1.0.0", "{type: directory, directory: ../../../../../etc}"),
        (
            "remote-pkg@1.0.0",
            "{integrity: sha512-deadbeef, tarball: 'file:../../../../../etc/a.tgz'}",
        ),
        (
            "remote-pkg@https://example.com/remote-pkg.tgz",
            "{type: directory, directory: ../../../../../etc}",
        ),
        (
            "remote-pkg@https://example.com/remote-pkg.tgz",
            "{integrity: sha512-deadbeef, tarball: 'file:../../../../../etc/a.tgz'}",
        ),
        ("foo@file:packages/foo", "{type: directory, directory: ../../../../../etc}"),
        (
            "foo@file:packages/foo.tgz",
            "{integrity: sha512-deadbeef, tarball: 'file:../../../../../etc/foo.tgz'}",
        ),
    ] {
        let importers = match pkg_key.strip_prefix("foo@") {
            Some(ver) => format!(
                "\nimporters:\n  .:\n    dependencies:\n      foo:\n        specifier: {ver}\n        version: {ver}\n",
            ),
            None => String::new(),
        };
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'\n{importers}\npackages:\n  '{pkg_key}':\n    resolution: {resolution}\n\nsnapshots:\n  '{pkg_key}': {{}}\n",
        ));
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .expect_err("local resolution under forged key must be rejected");
        assert!(
            matches!(err, VerifyError::ResolutionShapeMismatch { .. }),
            "expected ResolutionShapeMismatch for {pkg_key} -> {resolution}, got {err:?}",
        );
    }
}

#[tokio::test]
async fn rejects_non_local_snapshot_referencing_unauthorized_local_or_link_children() {
    for child_yaml in [
        "packages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  host-etc@file:../../../../../etc:\n    resolution: {type: directory, directory: ../../../../../etc}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-etc: file:../../../../../etc\n",
        "packages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  host-sub@file:sub:\n    resolution: {type: directory, directory: sub}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-sub: file:sub\n",
        "packages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-link: link:../../../../../etc\n",
        "packages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-link: link:<root>/../../etc\n",
    ] {
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n{child_yaml}",
        ));
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .expect_err("unauthorized local child on non-local snapshot must fail");
        assert!(
            matches!(err, VerifyError::LocalSubdepNotAllowed { .. }),
            "expected LocalSubdepNotAllowed, got {err:?}",
        );
    }
}

#[tokio::test]
async fn ignores_forged_lockfile_overrides_when_active_local_policy_disallows_hooks() {
    let lockfile = parse(
        "lockfileVersion: '9.0'
overrides:
  host-etc: file:../../../../../etc
importers:
  .:
    dependencies:
      remote-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  remote-pkg@1.0.0:
    resolution: {integrity: sha512-deadbeef}
  host-etc@file:../../../../../etc:
    resolution: {type: directory, directory: ../../../../../etc}
snapshots:
  remote-pkg@1.0.0:
    dependencies:
      host-etc: file:../../../../../etc
",
    );
    let opts = VerifyLockfileResolutionsOptions {
        local_policy: Some(LocalResolutionPolicy::default()),
        ..VerifyLockfileResolutionsOptions::default()
    };
    let err = verify_lockfile_resolutions::<SilentReporter>(&lockfile, &[], &opts)
        .await
        .expect_err("forged lockfile overrides must not bypass active policy");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }), "got {err:?}");
}

#[tokio::test]
async fn allows_package_root_links_importer_local_deps_workspace_members_and_active_overrides() {
    let valid_lockfile = parse(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      remote-pkg:
        specifier: 1.0.0
        version: 1.0.0
      vendor-lib:
        specifier: file:../vendor/lib
        version: file:../vendor/lib(react@18.0.0)
  packages/shared: {}
packages:
  remote-pkg@1.0.0:
    resolution: {integrity: sha512-deadbeef}
  vendor-lib@file:../vendor/lib:
    resolution: {type: directory, directory: ../vendor/lib}
  vendor-sub@file:../vendor/sub:
    resolution: {type: directory, directory: ../vendor/sub}
snapshots:
  remote-pkg@1.0.0:
    dependencies:
      internal-child: link:<root>/child
      shared: link:packages/shared
  vendor-lib@file:../vendor/lib(react@18.0.0):
    dependencies:
      vendor-sub: file:../vendor/sub
  vendor-sub@file:../vendor/sub: {}
",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &valid_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(LocalResolutionPolicy::default()),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("legitimate internal links, importer local deps, and workspace importers must pass");

    let overridden_lockfile = parse(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      remote-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  remote-pkg@1.0.0:
    resolution: {integrity: sha512-deadbeef}
  helper@file:local-helper:
    resolution: {type: directory, directory: local-helper}
snapshots:
  remote-pkg@1.0.0:
    dependencies:
      helper: file:local-helper
  helper@file:local-helper: {}
",
    );
    let override_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper", "file:./local-helper")],
        [],
        false,
        None,
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &overridden_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(override_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("active workspace local override must pass");
}

#[tokio::test]
async fn rejects_root_slash_links_deploy_keys_outside_deploy_and_unscoped_borrowing() {
    for (label, yaml) in [
        (
            "link-to-host-root",
            "lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      root-leak: link:/\n",
        ),
        (
            "link-to-absolute-workspace-name",
            "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/shared: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      abs-leak: link:/packages/shared\n",
        ),
        (
            "deploy-file-url-outside-deploy",
            "lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  secret@file:///tmp/secret:\n    resolution: {type: directory, directory: /tmp/secret}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      secret: file:///tmp/secret\n  secret@file:///tmp/secret: {}\n",
        ),
        (
            "borrowed-importer-link-by-non-peer",
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      peer-c:\n        specifier: link:../peer-c\n        version: link:../peer-c\n      remote:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      peer-c: link:../peer-c\n",
        ),
        (
            "unreachable-decoy-local-peer",
            "lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  decoy@file:./decoy:\n    resolution: {type: directory, directory: ./decoy}\n    peerDependencies:\n      secret: file:../secret\nsnapshots:\n  decoy@file:./decoy:\n    dependencies:\n      secret: file:../secret\n",
        ),
    ] {
        let lockfile = parse(yaml);
        let policy = LocalResolutionPolicy::from_scoped_inputs(
            [("trusted-parent>helper", "file:./local-helper"), ("ws-dep", "workspace:*")],
            [],
            true,
            None,
        );
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(policy),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
        .expect_err(label);
        assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }), "{label}: {err:?}");
    }
}

#[tokio::test]
async fn rejects_escaping_file_key_target_substitution_and_variations_escape() {
    for (label, yaml) in [
        (
            "substituted-importer-file-target",
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      vendor-lib:\n        specifier: file:../vendor/lib\n        version: file:../vendor/lib\npackages:\n  vendor-lib@file:../vendor/lib:\n    resolution: {type: directory, directory: /etc}\nsnapshots:\n  vendor-lib@file:../vendor/lib: {}\n",
        ),
        (
            "file-key-with-https-tarball-and-link-child",
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n       disguised:\n        specifier: https://example.com/disguised.tgz\n        version: file:./disguised\npackages:\n  disguised@file:./disguised:\n    resolution: {tarball: https://example.com/disguised.tgz}\nsnapshots:\n  disguised@file:./disguised:\n    dependencies:\n      leak: link:../secret\n",
        ),
        (
            "variations-escaping-directory",
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      var-pkg:\n        specifier: file:./local\n        version: file:./local\npackages:\n  var-pkg@file:./local:\n    resolution:\n      type: variations\n      variants:\n        - targets: [{os: linux, cpu: x64}]\n          resolution: {type: directory, directory: ../../../etc}\nsnapshots:\n  var-pkg@file:./local: {}\n",
        ),
    ] {
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &parse(yaml),
            &[],
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .expect_err(label);
        assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "{label}: {err:?}");
    }
}

#[tokio::test]
async fn authorizes_transitive_local_closure_of_override_regardless_of_snapshot_key_hash_order() {
    for helper_name in ["aaa-helper", "zzz-helper"] {
        let yaml = format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {{integrity: sha512-deadbeef}}\n  {helper_name}@file:local-helper:\n    resolution: {{type: directory, directory: local-helper}}\n  nested-out@file:../outside-nested:\n    resolution: {{type: directory, directory: ../outside-nested}}\nsnapshots:\n  {helper_name}@file:local-helper:\n    dependencies:\n      nested-out: file:../outside-nested\n      ext-link: link:../outside-link\n  nested-out@file:../outside-nested: {{}}\n  remote-pkg@1.0.0:\n    dependencies:\n      {helper_name}: file:local-helper\n",
        );
        let lockfile = parse(&yaml);
        let selector = format!("remote-pkg>{helper_name}");
        let allowed = LocalResolutionPolicy::from_scoped_inputs(
            [(selector.as_str(), "file:./local-helper")],
            [],
            false,
            None,
        );
        verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(allowed),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
        .expect("override-authorized helper and its nested local children must pass");

        let unmatched_sel = format!("other-parent>{helper_name}");
        let unmatched = LocalResolutionPolicy::from_scoped_inputs(
            [(unmatched_sel.as_str(), "file:./local-helper")],
            [],
            false,
            None,
        );
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(unmatched),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
        .expect_err("unmatched override must still reject helper and its children");
        assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
    }
}

#[tokio::test]
async fn enforces_key_semver_ranged_child_overrides_mixed_variations_and_live_workspace_roots() {
    let spoofed_ver = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      parent:\n        specifier: 2.0.0\n        version: 2.0.0\npackages:\n  parent@2.0.0:\n    version: 1.0.0\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:../outside:\n    resolution: {type: directory, directory: ../outside}\nsnapshots:\n  parent@2.0.0:\n    dependencies:\n      helper: file:../outside\n  helper@file:../outside: {}\n",
    );
    let parent_v1_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("parent@^1>helper", "file:../outside")],
        [],
        false,
        None,
    );
    assert!(
        verify_lockfile_local_resolutions(&spoofed_ver, Some(parent_v1_policy)).is_err(),
        "key semver 2.0.0 must not match parent@^1 via spoofed metadata.version",
    );

    let widget_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  widget@file:helper:\n    resolution: {type: directory, directory: helper}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      widget: file:helper\n  widget@file:helper: {}\n",
    );
    let ranged_child = LocalResolutionPolicy::from_scoped_inputs(
        [("widget@^1", "file:C:/work/app/helper")],
        [],
        false,
        Some(std::path::Path::new("C:/work/app")),
    );
    assert!(
        verify_lockfile_local_resolutions(&widget_lockfile, Some(ranged_child)).is_err(),
        "ranged child override without declared range proof must be rejected",
    );
    let unconditional_drive = LocalResolutionPolicy::from_scoped_inputs(
        [("widget", "file:C:/work/app/helper")],
        [],
        false,
        Some(std::path::Path::new("C:/work/app")),
    );
    verify_lockfile_local_resolutions(&widget_lockfile, Some(unconditional_drive))
        .expect("unconditional Windows drive-qualified override must match relative target");

    let mixed_var = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      mixed:\n        specifier: file:./local\n        version: file:./local\npackages:\n  mixed@file:./local:\n    resolution:\n      type: variations\n      variants:\n        - targets: [{os: linux, cpu: x64}]\n          resolution: {type: directory, directory: ./local}\n        - targets: [{os: darwin, cpu: arm64}]\n          resolution: {tarball: https://evil.example/mixed.tgz}\nsnapshots:\n  mixed@file:./local:\n    dependencies:\n      leak: link:../secret\n",
    );
    assert!(
        verify_lockfile_local_resolutions(&mixed_var, None).is_err(),
        "mixed directory + remote tarball variations must not skip child link checks",
    );

    let forged_importer = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/forged: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      forged: link:packages/forged\n",
    );
    let live_policy = LocalResolutionPolicy::default().with_live_workspace_dirs([".".to_string()]);
    assert!(
        verify_lockfile_local_resolutions(&forged_importer, Some(live_policy)).is_err(),
        "forged lockfile importer absent from live_workspace_dirs must be rejected",
    );
}
