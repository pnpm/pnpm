use axum::body::to_bytes;
use miette::IntoDiagnostic as _;
use pnpr_config::{Config, HostedStoreConfig, S3Settings};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::Storage;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Default)]
pub struct MigrateStorageOptions {
    pub from: Option<PathBuf>,
    pub to_bucket: Option<String>,
    pub to_region: Option<String>,
    pub to_endpoint: Option<String>,
    pub to_prefix: Option<String>,
    pub dry_run: bool,
    pub verify: bool,
}

#[derive(Debug, Default)]
pub struct MigrateReport {
    pub packages_migrated: usize,
    pub blobs_transferred: usize,
    pub bytes_transferred: u64,
    pub dry_run: bool,
}

pub async fn migrate_local_to_s3(
    config: &Config,
    options: &MigrateStorageOptions,
) -> miette::Result<MigrateReport> {
    let source_dir =
        options.from.clone().unwrap_or_else(|| config.storage.hosted_dir.clone());

    if !source_dir.exists() {
        let display_source = source_dir.display();
        return Err(miette::miette!("source storage directory '{display_source}' does not exist"));
    }

    let target_hosted_config = resolve_target_s3_config(config, options)?;

    let source = Storage::new(&HostedStoreConfig::Fs, source_dir, config.storage.cache_dir.clone())
        .map_err(|err| miette::miette!("failed to initialize source storage: {err}"))?;

    let target = Storage::new(
        &target_hosted_config,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )
    .map_err(|err| miette::miette!("failed to initialize target S3 storage: {err}"))?;

    let mut report = MigrateReport { dry_run: options.dry_run, ..Default::default() };

    let package_names = source
        .hosted_package_names()
        .await
        .map_err(|err| miette::miette!("failed to list source packages: {err}"))?;

    for pkg_name in package_names {
        migrate_package(&source, &target, &pkg_name, options, &mut report).await?;
    }

    Ok(report)
}

fn resolve_target_s3_config(
    config: &Config,
    options: &MigrateStorageOptions,
) -> miette::Result<HostedStoreConfig> {
    if let Some(bucket) = &options.to_bucket {
        let settings = S3Settings {
            bucket: bucket.clone(),
            region: options.to_region.clone(),
            endpoint: options.to_endpoint.clone(),
            prefix: options.to_prefix.clone(),
            access_key_id: None,
            secret_access_key: None,
            force_path_style: Some(false),
            allow_http: Some(false),
        };
        Ok(HostedStoreConfig::S3(settings))
    } else if let HostedStoreConfig::S3(s) = &config.storage.hosted_backend {
        Ok(HostedStoreConfig::S3(s.clone()))
    } else {
        Err(miette::miette!(
            "destination S3 storage must be configured via YAML 's3:' or CLI flag '--to-bucket'"
        ))
    }
}

async fn migrate_package(
    source: &Storage,
    target: &Storage,
    pkg_name: &str,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let canonical = CanonicalPackageName::parse(pkg_name, Ecosystem::Npm)
        .map_err(|err| miette::miette!("invalid package name '{pkg_name}': {err}"))?;

    let doc_bytes = source
        .read_hosted_document(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to read document for '{pkg_name}': {err}"))?;

    let Some(bytes) = doc_bytes else {
        return Ok(());
    };

    if !options.dry_run {
        target
            .write_hosted_document_if_current(&canonical, &bytes, None)
            .await
            .map_err(|err| miette::miette!("failed to write document to target: {err}"))?;
    }
    report.packages_migrated += 1;

    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|err| miette::miette!("invalid JSON in '{pkg_name}': {err}"))?;

    if let Some(versions) = doc.get("versions").and_then(Value::as_object) {
        for ver_val in versions.values() {
            migrate_version_blobs(source, target, &canonical, ver_val, options, report).await?;
        }
    }
    Ok(())
}

async fn migrate_version_blobs(
    source: &Storage,
    target: &Storage,
    canonical: &CanonicalPackageName,
    ver_val: &Value,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let Some(dist) = ver_val.get("dist").and_then(Value::as_object) else {
        return Ok(());
    };
    if let Some(filename) = dist
        .get("tarball")
        .and_then(Value::as_str)
        .and_then(filename_from_url)
    {
        migrate_single_blob(source, target, canonical, filename, options, report).await?;
    }
    if let Some(filename) = dist
        .get("attestations")
        .and_then(|a| a.get("url"))
        .and_then(Value::as_str)
        .and_then(filename_from_url)
    {
        migrate_single_blob(source, target, canonical, filename, options, report).await?;
    }
    Ok(())
}

async fn migrate_single_blob(
    source: &Storage,
    target: &Storage,
    canonical: &CanonicalPackageName,
    filename: &str,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let opened = source
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("failed to read source blob '{filename}': {err}"))?;

    let Some((body, _)) = opened else {
        return Ok(());
    };

    let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
    report.blobs_transferred += 1;
    report.bytes_transferred += bytes.len() as u64;

    if !options.dry_run {
        let slot = target
            .reserve_hosted_blob(canonical, filename)
            .await
            .map_err(|err| miette::miette!("failed to reserve blob slot on target: {err}"))?;

        fs::write(&slot.tmp_path, &bytes).await.into_diagnostic()?;

        target
            .finalize_blob_slot(slot)
            .await
            .map_err(|err| miette::miette!("failed to finalize blob on target: {err}"))?;

        if options.verify {
            verify_uploaded_blob(target, canonical, filename, bytes.len() as u64).await?;
        }
    }
    Ok(())
}

async fn verify_uploaded_blob(
    target: &Storage,
    canonical: &CanonicalPackageName,
    filename: &str,
    expected_size: u64,
) -> miette::Result<()> {
    let opened = target
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("failed to verify uploaded blob '{filename}': {err}"))?;

    match opened {
        Some((_, Some(size))) if size == expected_size => Ok(()),
        Some((body, _)) => {
            let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
            if bytes.len() as u64 == expected_size {
                Ok(())
            } else {
                Err(miette::miette!(
                    "uploaded blob '{filename}' size mismatch: expected {expected_size}, got {}",
                    bytes.len()
                ))
            }
        }
        None => Err(miette::miette!("uploaded blob '{filename}' not found in target store")),
    }
}

fn filename_from_url(url: &str) -> Option<&str> {
    url.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
}

#[must_use]
pub fn format_migrate_report(report: &MigrateReport) -> String {
    let action = if report.dry_run { "would be migrated" } else { "migrated" };
    format!(
        "Storage migration completed: {} packages, {} blobs ({} bytes) {}\n",
        report.packages_migrated, report.blobs_transferred, report.bytes_transferred, action,
    )
}
