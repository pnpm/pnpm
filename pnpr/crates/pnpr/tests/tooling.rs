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
    };
    let dry_rep = import_htpasswd(&dry_run_opts).unwrap();
    assert_eq!(dry_rep.merged, 2);
    assert_eq!(dry_rep.sha_count, 1);
    assert_eq!(dry_rep.bcrypt_count, 1);
    assert!(!target.exists());

    // 2. Real import
    let real_opts = HtpasswdImportOptions { source, target: Some(target.clone()), dry_run: false };
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
    let pkg_dir = config.storage.hosted_dir.join("demo-pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
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
    std::fs::write(pkg_dir.join("package.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    std::fs::write(pkg_dir.join("demo-pkg-1.0.0.tgz"), b"fake tarball bytes").unwrap();

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

    let pkg_dir = config.storage.hosted_dir.join("demo-pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();

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
    std::fs::write(pkg_dir.join("package.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    std::fs::write(pkg_dir.join("demo-pkg-1.0.0.tgz"), tarball_bytes).unwrap();

    // Add an orphaned blob
    let orphan_path = pkg_dir.join("orphaned-blob.tgz");
    std::fs::write(&orphan_path, b"unreferenced").unwrap();

    // 1. Verify detecting the orphan
    let opts = VerifyOptions { check_hashes: true, prune_orphans: false };
    let report = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(report.packages_checked, 1);
    assert_eq!(report.versions_checked, 1);
    assert_eq!(report.blobs_checked, 1);
    assert!(report.missing_blobs.is_empty());
    assert!(report.corrupted_blobs.is_empty());
    assert_eq!(report.orphaned_blobs.len(), 1);
    assert!(orphan_path.exists());

    // 2. Verify with prune_orphans: true
    let prune_opts = VerifyOptions { check_hashes: true, prune_orphans: true };
    let prune_report = verify_storage(&config, &prune_opts).await.unwrap();
    assert_eq!(prune_report.orphaned_blobs.len(), 1);
    assert!(!orphan_path.exists());
}

#[tokio::test]
async fn test_verify_storage_corrupted_and_missing() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());

    let pkg_dir = config.storage.hosted_dir.join("demo-pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();

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
    std::fs::write(pkg_dir.join("package.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    std::fs::write(pkg_dir.join("corrupt-1.0.0.tgz"), b"some mismatched content").unwrap();

    let opts = VerifyOptions { check_hashes: true, prune_orphans: false };
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

    let pkg_dir = config.storage.hosted_dir.join("demo-pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
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
    std::fs::write(pkg_dir.join("package.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    std::fs::write(pkg_dir.join("demo-pkg-1.0.0.tgz"), b"fake tarball bytes").unwrap();

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
