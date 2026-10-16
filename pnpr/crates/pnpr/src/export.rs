use axum::body::to_bytes;
use miette::IntoDiagnostic as _;
use pnpr_config::Config;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::Storage;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Default)]
pub struct ExportReport {
    pub packages_exported: usize,
    pub versions_exported: usize,
    pub blobs_exported: usize,
    pub bytes_exported: u64,
    pub out_dir: PathBuf,
}

pub async fn export_storage(
    config: &Config,
    out: &Path,
    include_auth: bool,
) -> miette::Result<ExportReport> {
    fs::create_dir_all(out).await.into_diagnostic()?;

    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )
    .map_err(|err| miette::miette!("failed to initialize storage: {err}"))?;

    let mut report = ExportReport { out_dir: out.to_path_buf(), ..Default::default() };

    let package_names = storage
        .hosted_package_names()
        .await
        .map_err(|err| miette::miette!("failed to list hosted packages: {err}"))?;

    for pkg_name in package_names {
        export_package(&storage, &pkg_name, out, &mut report).await?;
    }

    if include_auth {
        export_auth_files(config, out).await?;
    }

    Ok(report)
}

async fn export_package(
    storage: &Storage,
    pkg_name: &str,
    out: &Path,
    report: &mut ExportReport,
) -> miette::Result<()> {
    let canonical = CanonicalPackageName::parse(pkg_name, Ecosystem::Npm)
        .map_err(|err| miette::miette!("invalid package name '{pkg_name}': {err}"))?;

    let doc_bytes = storage
        .read_hosted_document(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to read document for '{pkg_name}': {err}"))?;

    let Some(bytes) = doc_bytes else {
        return Ok(());
    };

    let pkg_dir = out.join(pkg_name);
    fs::create_dir_all(&pkg_dir).await.into_diagnostic()?;
    fs::write(pkg_dir.join("package.json"), &bytes).await.into_diagnostic()?;
    report.packages_exported += 1;

    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|err| miette::miette!("invalid JSON in '{pkg_name}': {err}"))?;

    if let Some(versions) = doc.get("versions").and_then(Value::as_object) {
        for ver_val in versions.values() {
            report.versions_exported += 1;
            export_version_blobs(storage, &canonical, ver_val, &pkg_dir, report).await?;
        }
    }
    Ok(())
}

async fn export_version_blobs(
    storage: &Storage,
    canonical: &CanonicalPackageName,
    ver_val: &Value,
    pkg_dir: &Path,
    report: &mut ExportReport,
) -> miette::Result<()> {
    let Some(dist) = ver_val.get("dist").and_then(Value::as_object) else {
        return Ok(());
    };
    if let Some(filename) = dist
        .get("tarball")
        .and_then(Value::as_str)
        .and_then(filename_from_url)
    {
        export_single_blob(storage, canonical, filename, pkg_dir, report).await?;
    }
    if let Some(filename) = dist
        .get("attestations")
        .and_then(|a| a.get("url"))
        .and_then(Value::as_str)
        .and_then(filename_from_url)
    {
        export_single_blob(storage, canonical, filename, pkg_dir, report).await?;
    }
    Ok(())
}

async fn export_single_blob(
    storage: &Storage,
    canonical: &CanonicalPackageName,
    filename: &str,
    pkg_dir: &Path,
    report: &mut ExportReport,
) -> miette::Result<()> {
    let dest_file = pkg_dir.join(filename);
    if dest_file.exists() {
        return Ok(());
    }

    let opened = storage
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("failed to read blob '{filename}': {err}"))?;

    if let Some((body, _)) = opened {
        let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
        fs::write(&dest_file, &bytes).await.into_diagnostic()?;
        report.blobs_exported += 1;
        report.bytes_exported += bytes.len() as u64;
    }
    Ok(())
}

async fn export_auth_files(config: &Config, out: &Path) -> miette::Result<()> {
    if let Some(ht_path) = config.identity.auth.htpasswd.file.as_deref().filter(|p| p.exists()) {
        fs::copy(ht_path, out.join("htpasswd")).await.into_diagnostic()?;
    }
    if let Some(tok_path) = config.identity.auth.tokens.file.as_deref().filter(|p| p.exists()) {
        fs::copy(tok_path, out.join("tokens.db")).await.into_diagnostic()?;
    }
    Ok(())
}

fn filename_from_url(url: &str) -> Option<&str> {
    url.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
}

#[must_use]
pub fn format_export_report(report: &ExportReport) -> String {
    let display_out = report.out_dir.display();
    format!(
        "Export completed successfully:\n  Packages: {}\n  Versions: {}\n  Blobs: {} ({} bytes)\n  Destination: {}\n",
        report.packages_exported,
        report.versions_exported,
        report.blobs_exported,
        report.bytes_exported,
        display_out,
    )
}
