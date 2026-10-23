use std::path::Path;
use tempfile::TempDir;

use pnpr::{
    Config,
    config_check::{check_config, format_report},
    export::{export_storage, format_export_report},
    htpasswd_import::{HtpasswdImportOptions, import_htpasswd},
    migrate_storage::{MigrateStorageOptions, format_migrate_report, migrate_local_to_s3},
    verify::{VerifyOptions, format_verify_report, verify_storage},
};
use pnpr_storage::layout::{local_blob_path, local_document_path, write_local_layout_marker};

/// Write `bytes` to `path` in a hosted store, creating its directories.
fn seed_file(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn create_test_config(dir: &Path) -> (Config, tempfile::NamedTempFile) {
    let storage_dir = dir.join("storage");
    let cache_dir = dir.join("cache");
    let htpasswd_file = dir.join("htpasswd");
    std::fs::create_dir_all(&storage_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(&htpasswd_file, "").unwrap();

    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
auth:
  htpasswd:
    file: '{}'
",
        storage_dir.display(),
        cache_dir.display(),
        htpasswd_file.display(),
    );

    let config_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(config_file.path(), yaml).unwrap();
    let config =
        Config::from_yaml(config_file.path(), "127.0.0.1:7677".parse().unwrap(), None).unwrap();
    (config, config_file)
}

#[tokio::test]
async fn test_config_check_success() {
    let report = check_config(None).expect("bundled config check should succeed");
    let formatted = format_report(&report);
    assert!(formatted.contains("Configuration check: OK"));
    assert!(!report.registries.is_empty());
}

#[tokio::test]
async fn test_config_check_nonexistent() {
    let err = check_config(Some(Path::new("/nonexistent/pnpr/config.yaml"))).unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[tokio::test]
async fn test_htpasswd_import() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source-htpasswd");
    let target = dir.path().join("target-htpasswd");

    let source_content =
        "user1:{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g=\nuser2:$2y$10$abcdefghijklmnopqrstuu\n";
    std::fs::write(&source, source_content).unwrap();

    // 1. Dry run
    let dry_run_opts = HtpasswdImportOptions {
        source: source.clone(),
        target: Some(target.clone()),
        dry_run: true,
        overwrite: false,
    };
    let dry_rep = import_htpasswd(&dry_run_opts).unwrap();
    assert_eq!(dry_rep.merged, 2);
    assert_eq!(dry_rep.sha_count, 1);
    assert_eq!(dry_rep.bcrypt_count, 1);
    assert!(!target.exists());

    // 2. Real import
    let real_opts = HtpasswdImportOptions {
        source,
        target: Some(target.clone()),
        dry_run: false,
        overwrite: false,
    };
    let real_rep = import_htpasswd(&real_opts).unwrap();
    assert_eq!(real_rep.merged, 2);
    assert!(target.exists());

    let target_content = std::fs::read_to_string(&target).unwrap();
    assert!(target_content.contains("user1:{SHA}"));
    assert!(target_content.contains("user2:$2y$"));
}

#[tokio::test]
async fn test_export_storage() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());
    let export_out = dir.path().join("exported");

    // Seed a package in storage
    let pkg = "demo-pkg";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let doc = serde_json::json!({
        "name": "demo-pkg",
        "versions": {
            "1.0.0": {
                "name": "demo-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/demo-pkg/-/demo-pkg-1.0.0.tgz"
                }
            }
        }
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "demo-pkg-1.0.0.tgz"),
        b"fake tarball bytes",
    );

    let report = export_storage(&config, &export_out, true).await.unwrap();
    assert_eq!(report.packages_exported, 1);
    assert_eq!(report.versions_exported, 1);
    assert_eq!(report.blobs_exported, 1);

    let formatted = format_export_report(&report);
    assert!(formatted.contains("Export completed successfully"));
    assert!(export_out.join("demo-pkg/package.json").exists());
    assert!(export_out.join("demo-pkg/demo-pkg-1.0.0.tgz").exists());
}

#[tokio::test]
async fn test_verify_storage_health_and_orphan_prune() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg = "demo-pkg";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();

    let tarball_bytes = b"hello world tarball";
    let shasum = "18db23e4b48b9af2a99a7b2c9a4a4c9143380a18"; // sha1 of "hello world tarball"
    let doc = serde_json::json!({
        "name": "demo-pkg",
        "versions": {
            "1.0.0": {
                "name": "demo-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/demo-pkg/-/demo-pkg-1.0.0.tgz",
                    "shasum": shasum
                }
            }
        }
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "demo-pkg-1.0.0.tgz"),
        tarball_bytes,
    );

    // Add an orphaned blob
    let orphan_path = local_blob_path(&config.storage.hosted_dir, "", pkg, "orphaned-blob.tgz");
    std::fs::write(&orphan_path, b"unreferenced").unwrap();

    // 1. Verify detecting the orphan even with min_age_secs: 86400 (audit reports all orphans)
    let opts = VerifyOptions { check_hashes: true, prune_orphans: false, min_age_secs: 86400 };
    let report = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(report.packages_checked, 1);
    assert_eq!(report.versions_checked, 1);
    assert_eq!(report.blobs_checked, 1);
    assert!(report.missing_blobs.is_empty());
    assert!(report.corrupted_blobs.is_empty());
    assert_eq!(report.orphaned_blobs.len(), 1);
    assert!(orphan_path.exists());

    // 2. Prune with min_age_secs: 86400 does NOT prune freshly created orphan
    let guard_opts = VerifyOptions { check_hashes: true, prune_orphans: true, min_age_secs: 86400 };
    let guard_report = verify_storage(&config, &guard_opts).await.unwrap();
    assert_eq!(guard_report.orphaned_blobs.len(), 0);
    assert!(orphan_path.exists());

    // 3. Verify with prune_orphans: true and min_age_secs: 0 prunes the orphan
    let prune_opts = VerifyOptions { check_hashes: true, prune_orphans: true, min_age_secs: 0 };
    let prune_report = verify_storage(&config, &prune_opts).await.unwrap();
    assert_eq!(prune_report.orphaned_blobs.len(), 1);
    assert!(prune_report.prune_failures.is_empty());
    assert!(!orphan_path.exists());

    // 4. Verify an undeclared .sigstore file is pruned when dist.attestations is absent
    let undeclared_sigstore =
        local_blob_path(&config.storage.hosted_dir, "", pkg, "demo-pkg-1.0.0.sigstore");
    std::fs::write(&undeclared_sigstore, b"abandoned sigstore").unwrap();
    let sigstore_report = verify_storage(&config, &prune_opts).await.unwrap();
    assert_eq!(sigstore_report.orphaned_blobs.len(), 1);
    assert!(!undeclared_sigstore.exists());
}

#[tokio::test]
async fn test_verify_storage_corrupted_and_missing() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg = "demo-pkg";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();

    let doc = serde_json::json!({
        "name": "demo-pkg",
        "versions": {
            "1.0.0": {
                "name": "demo-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/demo-pkg/-/corrupt-1.0.0.tgz",
                    "shasum": "0000000000000000000000000000000000000000"
                }
            },
            "2.0.0": {
                "name": "demo-pkg",
                "version": "2.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/demo-pkg/-/missing-2.0.0.tgz"
                }
            }
        }
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "corrupt-1.0.0.tgz"),
        b"some mismatched content",
    );

    let opts = VerifyOptions { check_hashes: true, prune_orphans: false, min_age_secs: 0 };
    let report = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(report.missing_blobs.len(), 1);
    assert_eq!(report.corrupted_blobs.len(), 1);

    let formatted = format_verify_report(&report);
    assert!(formatted.contains("MISSING"));
    assert!(formatted.contains("CORRUPTED"));
}

#[tokio::test]
async fn test_migrate_storage_dry_run() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg = "demo-pkg";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let doc = serde_json::json!({
        "name": "demo-pkg",
        "versions": {
            "1.0.0": {
                "name": "demo-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/demo-pkg/-/demo-pkg-1.0.0.tgz"
                }
            }
        }
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "demo-pkg-1.0.0.tgz"),
        b"fake tarball bytes",
    );

    let options = MigrateStorageOptions {
        from: None,
        to_bucket: Some("mock-bucket".to_string()),
        to_region: Some("us-east-1".to_string()),
        to_endpoint: None,
        to_prefix: None,
        dry_run: true,
        verify: false,
    };

    let report = migrate_local_to_s3(&config, &options).await.unwrap();
    assert_eq!(report.packages_migrated, 1);
    assert_eq!(report.blobs_transferred, 1);
    assert!(report.dry_run);

    let formatted = format_migrate_report(&report);
    assert!(formatted.contains("would be migrated"));
}

#[tokio::test]
async fn test_migrate_storage_non_npm_package() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg = "my-crate";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let doc = serde_json::json!({
        "format_version": 1,
        "name": "my-crate",
        "vers": [
            {
                "name": "my-crate",
                "vers": "0.1.0",
                "cksum": "abc"
            }
        ]
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "my-crate-0.1.0.crate"),
        b"fake crate bytes",
    );

    let options = MigrateStorageOptions {
        from: None,
        to_bucket: Some("mock-bucket".to_string()),
        to_region: Some("us-east-1".to_string()),
        to_endpoint: None,
        to_prefix: None,
        dry_run: true,
        verify: false,
    };

    let report = migrate_local_to_s3(&config, &options).await.unwrap();
    assert_eq!(report.packages_migrated, 1);
    assert_eq!(report.blobs_transferred, 1);
    assert!(report.dry_run);
}

#[tokio::test]
async fn test_verify_and_export_attestations() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg = "signed-pkg";
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();

    let tarball_bytes = b"signed package tarball";
    let shasum = "327001467a169ac289299b0a689cab5a19be6372";
    let doc = serde_json::json!({
        "name": "signed-pkg",
        "versions": {
            "1.0.0": {
                "name": "signed-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:7677/signed-pkg/-/signed-pkg-1.0.0.tgz",
                    "shasum": shasum,
                    "attestations": {
                        "url": "http://localhost:7677/-/npm/v1/attestations/signed-pkg@1.0.0"
                    }
                }
            }
        }
    });
    seed_file(
        local_document_path(&config.storage.hosted_dir, "", pkg),
        serde_json::to_vec(&doc).unwrap(),
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "signed-pkg-1.0.0.tgz"),
        tarball_bytes,
    );
    seed_file(
        local_blob_path(&config.storage.hosted_dir, "", pkg, "signed-pkg-1.0.0.sigstore"),
        b"sigstore bundle bytes",
    );

    let opts = VerifyOptions { check_hashes: true, prune_orphans: true, min_age_secs: 0 };
    let rep = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(rep.packages_checked, 1);
    assert_eq!(rep.missing_blobs.len(), 0);
    assert_eq!(rep.corrupted_blobs.len(), 0);
    assert_eq!(rep.orphaned_blobs.len(), 0);

    let export_out = dir.path().join("export-signed");
    let exp_rep = export_storage(&config, &export_out, false).await.unwrap();
    assert_eq!(exp_rep.packages_exported, 1);
    assert_eq!(exp_rep.blobs_exported, 2);
    assert!(export_out.join("signed-pkg/signed-pkg-1.0.0.tgz").exists());
    assert!(export_out.join("signed-pkg/signed-pkg-1.0.0.sigstore").exists());
    assert!(!export_out.join("htpasswd").exists());
}

#[test]
fn test_config_check_upstream_url_sanitized() {
    let dir = TempDir::new().unwrap();
    let storage_dir = dir.path().join("hosted");
    let cache_dir = dir.path().join("cache");
    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
registries:
  private:
    type: upstream
    url: 'https://secretuser:secretpass@registry.example.com/?auth_token=supersecret&sig=secretsig&X-Amz-Signature=amazonsig'
    public: true
",
        storage_dir.display(),
        cache_dir.display(),
    );

    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(&config_file, yaml).unwrap();

    let report = pnpr::config_check::check_config(Some(&config_file)).unwrap();
    let output = pnpr::config_check::format_report(&report);
    assert!(!output.contains("secretuser"));
    assert!(!output.contains("secretpass"));
    assert!(!output.contains("supersecret"));
    assert!(!output.contains("secretsig"));
    assert!(!output.contains("amazonsig"));
    assert!(output.contains("REDACTED"));
}

#[test]
fn test_config_check_reports_the_auth_backend_and_every_default_registry() {
    let dir = TempDir::new().unwrap();
    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
backend:
  libsql:
    url: 'libsql://db.example.turso.io'
    authToken: 'secret-database-token'
    replicaPath: '{}'
auth:
  htpasswd:
    max_users: 3
registries:
  local:
    type: hosted
    packages:
      '@acme/*':
        publish: $authenticated
  crates:
    type: hosted
    ecosystem: cargo
    org: crates
    packages:
      acme-crate:
        publish: $authenticated
defaultRegistry:
  npm: local
  cargo: crates
",
        dir.path().join("hosted").display(),
        dir.path().join("cache").display(),
        dir.path().join("replica.db").display(),
    );
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(&config_file, yaml).unwrap();

    let report = pnpr::config_check::check_config(Some(&config_file)).unwrap();
    let output = pnpr::config_check::format_report(&report);
    let replica = dir.path().join("replica.db");
    assert!(
        output.contains(&format!(
            "Auth: libsql (libsql://db.example.turso.io, replica: {}, registration: max 3)",
            replica.display(),
        )),
        "{output}",
    );
    assert!(!output.contains("secret-database-token"), "{output}");
    assert!(output.contains("Default registries: npm=local, cargo=crates"), "{output}");
}

#[test]
fn test_config_check_hides_an_unparsable_database_url() {
    let dir = TempDir::new().unwrap();
    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
backend:
  postgres:
    url: 'postgres://admin:hunter2@[::1'
registries:
  local:
    type: hosted
    packages:
      '@acme/*':
        publish: $authenticated
",
        dir.path().join("hosted").display(),
        dir.path().join("cache").display(),
    );
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(&config_file, yaml).unwrap();

    let report = pnpr::config_check::check_config(Some(&config_file)).unwrap();
    let output = pnpr::config_check::format_report(&report);
    assert!(output.contains("Auth: postgres (<unparsable URL>"), "{output}");
    assert!(!output.contains("hunter2"), "{output}");
}

#[test]
fn test_config_check_redacts_the_database_user_query_parameter() {
    let dir = TempDir::new().unwrap();
    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
backend:
  postgres:
    url: 'postgres://db.example.com/pnpr?user=registry-admin'
registries:
  local:
    type: hosted
    packages:
      '@acme/*':
        publish: $authenticated
",
        dir.path().join("hosted").display(),
        dir.path().join("cache").display(),
    );
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(&config_file, yaml).unwrap();

    let report = pnpr::config_check::check_config(Some(&config_file)).unwrap();
    let output = pnpr::config_check::format_report(&report);
    assert!(!output.contains("registry-admin"), "{output}");
    assert!(output.contains("user=REDACTED"), "{output}");
}

#[test]
fn test_htpasswd_import_malformed_and_collisions() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source-malformed");
    let target = dir.path().join("target-users");

    // 1. Malformed SHA hash is rejected
    std::fs::write(&source, "baduser:{SHA}not-valid-base64\n").unwrap();
    let opts = HtpasswdImportOptions {
        source: source.clone(),
        target: Some(target.clone()),
        dry_run: false,
        overwrite: false,
    };
    assert!(import_htpasswd(&opts).is_err());

    // 2. Existing user collision handling without and with overwrite
    std::fs::write(&target, "alice:$2y$10$abcdefghijklmnopqrstuu\n").unwrap();
    // Incoming source has alice with a different hash
    std::fs::write(
        &source,
        "alice:{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g=\nbob:$2y$10$abcdefghijklmnopqrstuu\n",
    )
    .unwrap();

    let no_ow_opts = HtpasswdImportOptions {
        source: source.clone(),
        target: Some(target.clone()),
        dry_run: false,
        overwrite: false,
    };
    let rep = import_htpasswd(&no_ow_opts).unwrap();
    assert_eq!(rep.merged, 1); // bob was merged
    assert_eq!(rep.collisions.len(), 1); // alice collided
    assert!(rep.collisions[0].contains("alice"));

    // Check alice was NOT overwritten
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(content.contains("alice:$2y$10$abcdefghijklmnopqrstuu"));

    // 3. With overwrite: true, alice gets replaced (bob is already identical)
    let ow_opts = HtpasswdImportOptions {
        source,
        target: Some(target.clone()),
        dry_run: false,
        overwrite: true,
    };
    let rep2 = import_htpasswd(&ow_opts).unwrap();
    assert_eq!(rep2.merged, 1);
    assert_eq!(rep2.collisions.len(), 0);

    let content2 = std::fs::read_to_string(&target).unwrap();
    assert!(content2.contains("alice:{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g="));
}
