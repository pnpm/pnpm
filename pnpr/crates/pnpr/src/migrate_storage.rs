use axum::body::to_bytes;
use miette::IntoDiagnostic as _;
use pnpr_config::{Config, HostedStoreConfig, MetadataBackend, S3Settings};
use pnpr_package_name::Ecosystem;
use pnpr_storage::{Storage, metadata::BlobRef};
use std::path::PathBuf;

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
    let (source, target) = init_source_and_target_storage(config, options)?;
    let mut report = MigrateReport { dry_run: options.dry_run, ..Default::default() };
    source
        .check_hosted_layout()
        .await
        .map_err(|err| miette::miette!("source: {err}"))?;
    if !options.dry_run {
        target
            .ensure_hosted_layout()
            .await
            .map_err(|err| miette::miette!("target: {err}"))?;
    }

    for org in crate::export::configured_hosted_namespaces(config) {
        for ecosystem in Ecosystem::all() {
            let src = source.for_hosted(&org, ecosystem);
            let tgt = target.for_hosted(&org, ecosystem);
            migrate_namespace(&src, &tgt, options, &mut report).await?;
        }
    }

    Ok(report)
}

/// The source store and the target store. Both keep their metadata in the
/// configured database: the command moves archives, and the metadata stays
/// where it is, so the S3 deployment has to read the same database.
fn init_source_and_target_storage(
    config: &Config,
    options: &MigrateStorageOptions,
) -> miette::Result<(Storage, Storage)> {
    let source_dir =
        options.from.clone().unwrap_or_else(|| config.storage.hosted_dir.clone());

    if !source_dir.exists() {
        let display_source = source_dir.display();
        return Err(miette::miette!("source storage directory '{display_source}' does not exist"));
    }
    if !options.dry_run && !matches!(config.storage.metadata, MetadataBackend::Libsql(_)) {
        return Err(miette::miette!(
            "migrate-storage moves archives and leaves package metadata in its database, which \
             an S3 deployment reads through a libSQL metadata.url. Move the metadata into the \
             libSQL database the S3 deployment will use, such as by importing the SQLite file, \
             and point metadata.url at it before migrating"
        ));
    }

    let target_hosted_config = resolve_target_s3_config(config, options)?;

    let source = Storage::new(
        &HostedStoreConfig::Fs,
        source_dir,
        config.storage.cache_dir.clone(),
        &config.storage.metadata,
    )
    .map_err(|err| miette::miette!("failed to initialize source storage: {err}"))?;

    let target = Storage::new(
        &target_hosted_config,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
        &config.storage.metadata,
    )
    .map_err(|err| miette::miette!("failed to initialize target S3 storage: {err}"))?;

    Ok((source, target))
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
            force_path_style: None,
            allow_http: None,
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

/// Copy every archive `source` stores to `target` under the same key: the
/// metadata both read maps the packages' files to those keys.
async fn migrate_namespace(
    source: &Storage,
    target: &Storage,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let packages = source
        .hosted_package_names()
        .await
        .map_err(|err| miette::miette!("failed to list source packages: {err}"))?;
    report.packages_migrated += packages.len();
    let stored = source
        .stored_incarnations()
        .await
        .map_err(|err| miette::miette!("failed to list source archives: {err}"))?;
    for (blob, _) in stored {
        migrate_archive(source, target, &blob, options, report).await?;
    }
    Ok(())
}

async fn migrate_archive(
    source: &Storage,
    target: &Storage,
    blob: &BlobRef,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let key = blob.key();
    let opened = source
        .open_stored_archive(blob)
        .await
        .map_err(|err| miette::miette!("failed to read source archive '{key}': {err}"))?;
    let Some((body, _)) = opened else { return Ok(()) };
    let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
    report.blobs_transferred += 1;
    report.bytes_transferred += bytes.len() as u64;
    if options.dry_run {
        return Ok(());
    }
    target
        .store_archive_copy(blob, &bytes)
        .await
        .map_err(|err| miette::miette!("failed to store archive '{key}' on target: {err}"))?;
    if options.verify {
        verify_uploaded_archive(target, blob, &bytes).await?;
    }
    Ok(())
}

async fn verify_uploaded_archive(
    target: &Storage,
    blob: &BlobRef,
    expected_bytes: &[u8],
) -> miette::Result<()> {
    let key = blob.key();
    let opened = target
        .open_stored_archive(blob)
        .await
        .map_err(|err| miette::miette!("failed to verify uploaded archive '{key}': {err}"))?;
    let Some((body, _)) = opened else {
        return Err(miette::miette!("uploaded archive '{key}' not found in target store"));
    };
    let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
    if bytes.as_ref() == expected_bytes {
        Ok(())
    } else {
        Err(miette::miette!(
            "uploaded archive '{key}' content mismatch: target does not match source"
        ))
    }
}

#[must_use]
pub fn format_migrate_report(report: &MigrateReport) -> String {
    let action = if report.dry_run { "would be migrated" } else { "migrated" };
    format!(
        "Storage migration completed: {} packages, {} blobs ({} bytes) {}\n",
        report.packages_migrated, report.blobs_transferred, report.bytes_transferred, action,
    )
}
