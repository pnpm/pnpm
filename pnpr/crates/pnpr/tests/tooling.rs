use std::{path::Path, time::Duration};
use tempfile::TempDir;

use pnpr::{
    Config,
    archive_gc::{GcOptions, GcReport, collect_archives},
    config_check::{check_config, format_report},
    export::{export_storage, format_export_report},
    htpasswd_import::{HtpasswdImportOptions, import_htpasswd},
    migrate_storage::{MigrateStorageOptions, format_migrate_report, migrate_local_to_s3},
    verify::{VerifyOptions, format_verify_report, verify_storage},
};
use pnpr_storage::{
    BlobId, Ecosystem,
    layout::{local_archive_path, write_local_layout_marker},
};

/// Commit `bytes` as the document of `name` in the default namespace of the
/// hosted store at `storage`, in the metadata a pnpr over it reads.
fn seed_document(storage: &Path, ecosystem: Ecosystem, name: &str, bytes: &[u8]) {
    pnpr_fixtures::write_hosted_document(storage, "", ecosystem, name, bytes);
}

/// Store `bytes` as the file `file` of `name` in the default namespace of
/// `ecosystem` of the hosted store at `storage`, mapped as a publish maps it.
fn seed_archive(storage: &Path, ecosystem: Ecosystem, name: &str, file: &str, bytes: &[u8]) {
    pnpr_fixtures::write_hosted_file(storage, ("", ecosystem), (name, ""), file, bytes);
}

/// Store `bytes` in the default npm namespace of the hosted store at
/// `storage` the way a publish refused at its commit leaves them: under the
/// first incarnation of their identity, with no row and nothing mapping them.
/// Returns where they are.
fn seed_uncommitted(storage: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let key = BlobId::of_bytes(Ecosystem::Npm, bytes).key(1);
    let path = local_archive_path(storage, "", Ecosystem::Npm, &key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn create_test_config(dir: &Path) -> (Config, tempfile::NamedTempFile) {
    create_test_config_with(dir, "")
}

/// [`create_test_config`] that hosts npm packages in the registry `local`.
fn create_hosting_test_config(dir: &Path) -> (Config, tempfile::NamedTempFile) {
    create_test_config_with(dir, "registries:\n  local:\n    type: hosted\n")
}

fn create_test_config_with(dir: &Path, registries: &str) -> (Config, tempfile::NamedTempFile) {
    let storage_dir = dir.join("storage");
    let cache_dir = dir.join("cache");
    let users_file = dir.join("users.json");
    std::fs::create_dir_all(&storage_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(&users_file, "{}").unwrap();

    let yaml = format!(
        r"
storage: '{}'
cache: '{}'
auth:
  users:
    file: '{}'
{registries}",
        storage_dir.display(),
        cache_dir.display(),
        users_file.display(),
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
    assert!(target_content.contains("{SHA}"));
    assert!(target_content.contains("$2y$"));
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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "demo-pkg-1.0.0.tgz",
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
async fn verify_and_export_keep_the_stored_tar_of_a_hosted_tar_registry() {
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
                "dist": { "tarball": "http://localhost:7677/demo-pkg/-/demo-pkg-1.0.0.tgz" }
            }
        }
    });
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    let tar = "demo-pkg-1.0.0.tar.zst";
    seed_archive(&config.storage.hosted_dir, Ecosystem::Npm, pkg, tar, b"zstd tar bytes");

    let opts = VerifyOptions { check_hashes: true };
    let report = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(report.blobs_checked, 1);
    assert!(report.missing_blobs.is_empty(), "{:?}", report.missing_blobs);
    assert!(report.orphaned_blobs.is_empty(), "{:?}", report.orphaned_blobs);
    let stored_tar =
        pnpr_fixtures::read_hosted_file(&config.storage.hosted_dir, ("", Ecosystem::Npm), pkg, tar);
    assert_eq!(stored_tar.as_deref(), Some(b"zstd tar bytes".as_slice()));
    assert_eq!(report.corrupted_blobs.len(), 1, "{:?}", report.corrupted_blobs);
    assert!(report.corrupted_blobs[0].contains("no recorded tar integrity"));

    let export_out = dir.path().join("exported");
    let exported = export_storage(&config, &export_out, true).await.unwrap();
    assert_eq!(exported.blobs_exported, 1);
    assert!(export_out.join("demo-pkg/demo-pkg-1.0.0.tar.zst").exists());
}

#[tokio::test]
async fn test_verify_reports_orphans_and_gc_removes_uncommitted_archives() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_hosting_test_config(dir.path());

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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "demo-pkg-1.0.0.tgz",
        tarball_bytes,
    );

    // A file pnpr never stores, and the bytes of a publish refused at its
    // commit.
    let hosted = &config.storage.hosted_dir;
    let unknown_path = local_archive_path(hosted, "", Ecosystem::Npm, "orphaned-blob.tgz");
    std::fs::create_dir_all(unknown_path.parent().unwrap()).unwrap();
    std::fs::write(&unknown_path, b"unreferenced").unwrap();
    let orphan_path = seed_uncommitted(hosted, b"refused at its commit");

    // Verify reports the uncommitted archive, leaves the name pnpr never
    // stored alone, and removes nothing.
    let opts = VerifyOptions { check_hashes: true };
    let report = verify_storage(&config, &opts).await.unwrap();
    assert_eq!(report.packages_checked, 1);
    assert_eq!(report.versions_checked, 1);
    assert_eq!(report.blobs_checked, 1);
    assert!(report.missing_blobs.is_empty());
    assert!(report.corrupted_blobs.is_empty());
    let orphan_key = orphan_path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(report.orphaned_blobs, [orphan_key], "{:?}", report.orphaned_blobs);
    assert!(orphan_path.exists() && unknown_path.exists());

    // An archive younger than the minimum age stays: a publish may still be
    // on its way to commit it.
    let mut gc = GcOptions { min_age: Duration::from_hours(24), ..GcOptions::default() };
    assert_eq!(collect_archives(&config, &gc).await.unwrap(), GcReport::default());
    assert!(orphan_path.exists());

    // A dry run reports what goes.
    gc.min_age = Duration::ZERO;
    gc.dry_run = true;
    assert_eq!(collect_archives(&config, &gc).await.unwrap().archives, 1);
    assert!(orphan_path.exists());

    // The uncommitted archive goes; the live one and the unknown file stay.
    gc.dry_run = false;
    assert_eq!(collect_archives(&config, &gc).await.unwrap().archives, 1);
    assert!(!orphan_path.exists());
    assert!(unknown_path.exists());
    let live =
        pnpr_fixtures::read_hosted_file(hosted, ("", Ecosystem::Npm), pkg, "demo-pkg-1.0.0.tgz");
    assert_eq!(live.as_deref(), Some(tarball_bytes.as_slice()));
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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "corrupt-1.0.0.tgz",
        b"some mismatched content",
    );

    let opts = VerifyOptions { check_hashes: true };
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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "demo-pkg-1.0.0.tgz",
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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Cargo,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Cargo,
        pkg,
        "my-crate-0.1.0.crate",
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

/// Write a one-version npm packument of each of `names` into `storage`.
async fn seed_packuments(storage: &pnpr_storage::Storage, names: &[&str]) {
    for name in names {
        let doc = serde_json::json!({
            "name": name,
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": name,
                    "version": "1.0.0",
                    "dist": { "integrity": "sha512-AAAA", "tarball": "t.tgz" },
                },
            },
        });
        let name = pnpr_package_name::CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap();
        let bytes = serde_json::to_vec(&doc).unwrap();
        storage.write_hosted_document_if_current(&name, &bytes, None, None).await.unwrap();
    }
}

/// The command moves archives, and package metadata stays in the configured
/// database. `--from` names where the archives are; the packages come from
/// that database.
#[tokio::test]
async fn test_migrate_storage_from_takes_the_archives_of_another_directory() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let storage = pnpr_storage::Storage::from_config(&config.storage).unwrap();
    seed_packuments(&storage, &["configured-a", "configured-b"]).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    write_local_layout_marker(&elsewhere).unwrap();
    seed_archive(&elsewhere, Ecosystem::Npm, "configured-a", "configured-a-1.0.0.tgz", b"tarball");

    let options = MigrateStorageOptions {
        from: Some(elsewhere),
        to_bucket: Some("mock-bucket".to_string()),
        to_region: Some("us-east-1".to_string()),
        dry_run: true,
        ..MigrateStorageOptions::default()
    };
    let report = migrate_local_to_s3(&config, &options).await.unwrap();
    assert_eq!(report.packages_migrated, 2);
    assert_eq!(report.blobs_transferred, 1);
}

/// An S3 deployment reads its metadata from a libSQL database, so a real
/// migration from `SQLite` metadata is refused before anything moves.
#[tokio::test]
async fn test_migrate_storage_refuses_metadata_an_s3_deployment_cannot_read() {
    let dir = TempDir::new().unwrap();
    let (config, _cfg_file) = create_test_config(dir.path());
    write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let options = MigrateStorageOptions {
        to_bucket: Some("mock-bucket".to_string()),
        to_region: Some("us-east-1".to_string()),
        ..MigrateStorageOptions::default()
    };
    let err = migrate_local_to_s3(&config, &options).await.unwrap_err();
    assert!(err.to_string().contains("libSQL metadata.url"), "{err}");
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
    seed_document(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        &serde_json::to_vec(&doc).unwrap(),
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "signed-pkg-1.0.0.tgz",
        tarball_bytes,
    );
    seed_archive(
        &config.storage.hosted_dir,
        Ecosystem::Npm,
        pkg,
        "signed-pkg-1.0.0.sigstore",
        b"sigstore bundle bytes",
    );

    let opts = VerifyOptions { check_hashes: true };
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
    assert!(!export_out.join("users.json").exists());
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
  users:
    maxUsers: 3
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
fn test_config_check_reports_the_s3_key_prefix() {
    let dir = TempDir::new().unwrap();
    let report = |prefix: &str| {
        let yaml = format!(
            r"
storage: '{}'
cache: '{}'
s3:
  bucket: packages
  region: auto
  endpoint: https://storage.example.com
{prefix}
metadata:
  url: libsql://db.example.turso.io
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
        pnpr::config_check::format_report(&report)
    };

    let output = report("  prefix: /pnpr");
    assert!(output.contains("Storage: S3 (bucket: packages, prefix: pnpr/)\n"), "{output}");
    let output = report("");
    assert!(output.contains("Storage: S3 (bucket: packages)\n"), "{output}");
    let output = report(r#"  prefix: "pn\npr""#);
    assert!(output.contains(r"Storage: S3 (bucket: packages, prefix: pn\npr/)"), "{output}");
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
    pnpr_auth::UserStore::import_password_hashes(
        &target,
        [("alice".to_string(), "$2y$10$abcdefghijklmnopqrstuu".to_string())].into(),
        pnpr_auth::PasswordImport {
            collisions: pnpr_auth::PasswordCollisions::Keep,
            mode: pnpr_auth::ImportMode::Persist,
        },
    )
    .unwrap();
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
    assert!(content.contains("$2y$10$abcdefghijklmnopqrstuu"));

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
    assert!(content2.contains("{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g="));
}

#[tokio::test]
async fn add_user_preserves_account_id_when_changing_password() {
    let dir = TempDir::new().unwrap();
    let (config, _config_file) = create_test_config(dir.path());
    pnpr::run_command(
        &config,
        pnpr::Command::AddUser {
            username: "example-user".to_string(),
            password: Some("first-password".to_string()),
        },
    )
    .await
    .unwrap();
    let auth = pnpr_auth::AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let id = auth.users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    pnpr::run_command(
        &config,
        pnpr::Command::AddUser {
            username: "example-user".to_string(),
            password: Some("second-password".to_string()),
        },
    )
    .await
    .unwrap();
    let reloaded = pnpr_auth::AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let (_, logged_in_id) =
        reloaded.users.add_or_login("example-user", "second-password").await.unwrap();
    assert_eq!(id, logged_in_id);
    assert!(reloaded.users.add_or_login("example-user", "first-password").await.is_err());
}
