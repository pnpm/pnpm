use axum::body::to_bytes;
use miette::IntoDiagnostic as _;
use pnpr_config::{Config, HostedStoreConfig, S3Settings};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::Storage;
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

    let namespaces = crate::export::configured_hosted_namespaces(config);
    for org in namespaces {
        let src = if org.is_empty() { source.clone() } else { source.for_hosted(&org) };
        let tgt = if org.is_empty() { target.clone() } else { target.for_hosted(&org) };

        let package_names = src
            .hosted_package_names()
            .await
            .map_err(|err| miette::miette!("failed to list source packages: {err}"))?;

        for pkg_name in package_names {
            migrate_package(&src, &tgt, &pkg_name, options, &mut report).await?;
        }
    }

    Ok(report)
}

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

    let target_hosted_config = resolve_target_s3_config(config, options)?;

    let source = Storage::new(&HostedStoreConfig::Fs, source_dir, config.storage.cache_dir.clone())
        .map_err(|err| miette::miette!("failed to initialize source storage: {err}"))?;

    let target = Storage::new(
        &target_hosted_config,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
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

async fn migrate_package(
    source: &Storage,
    target: &Storage,
    pkg_name: &str,
    options: &MigrateStorageOptions,
    report: &mut MigrateReport,
) -> miette::Result<()> {
    let canonical = CanonicalPackageName::from_stored(pkg_name);

    let doc_bytes = source
        .read_hosted_document(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to read document for '{pkg_name}': {err}"))?;

    if let Some(bytes) = doc_bytes {
        if !options.dry_run {
            target
                .write_hosted_document_if_current(&canonical, &bytes, None)
                .await
                .map_err(|err| miette::miette!("failed to write document to target: {err}"))?;
        }
        report.packages_migrated += 1;
    }

    let blobs = source
        .hosted_package_blobs(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to list blobs for '{pkg_name}': {err}"))?;

    for filename in blobs {
        migrate_single_blob(source, target, &canonical, &filename, options, report).await?;
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
    let Some(safe_name) = safe_filename(filename) else {
        return Ok(());
    };

    let opened = source
        .open_hosted_blob(canonical, safe_name)
        .await
        .map_err(|err| miette::miette!("failed to read source blob '{safe_name}': {err}"))?;

    let Some((body, _)) = opened else {
        return Ok(());
    };

    let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
    report.blobs_transferred += 1;
    report.bytes_transferred += bytes.len() as u64;

    if !options.dry_run {
        let slot = target
            .reserve_hosted_blob(canonical, safe_name)
            .await
            .map_err(|err| miette::miette!("failed to reserve blob slot on target: {err}"))?;

        fs::write(&slot.tmp_path, &bytes).await.into_diagnostic()?;

        let outcome = target
            .finalize_blob_slot(slot)
            .await
            .map_err(|err| miette::miette!("failed to finalize blob on target: {err}"))?;

        if outcome == pnpr_storage::BlobFinalize::Conflict {
            return Err(miette::miette!(
                "blob '{safe_name}' already exists on target storage with different content (conflict)"
            ));
        }

        if options.verify {
            verify_uploaded_blob(target, canonical, safe_name, &bytes).await?;
        }
    }
    Ok(())
}

async fn verify_uploaded_blob(
    target: &Storage,
    canonical: &CanonicalPackageName,
    filename: &str,
    expected_bytes: &[u8],
) -> miette::Result<()> {
    let opened = target
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("failed to verify uploaded blob '{filename}': {err}"))?;

    match opened {
        Some((body, _)) => {
            let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
            if bytes.as_ref() == expected_bytes {
                Ok(())
            } else {
                Err(miette::miette!(
                    "uploaded blob '{filename}' content mismatch: target does not match source"
                ))
            }
        }
        None => Err(miette::miette!("uploaded blob '{filename}' not found in target store")),
    }
}

fn safe_filename(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    let path = std::path::Path::new(name);
    if path.file_name().and_then(|f| f.to_str()) != Some(name) {
        return None;
    }
    Some(name)
}

#[must_use]
pub fn format_migrate_report(report: &MigrateReport) -> String {
    let action = if report.dry_run { "would be migrated" } else { "migrated" };
    format!(
        "Storage migration completed: {} packages, {} blobs ({} bytes) {}\n",
        report.packages_migrated, report.blobs_transferred, report.bytes_transferred, action,
    )
}
