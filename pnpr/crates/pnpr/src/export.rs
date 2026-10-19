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

    let storage = open_hosted_storage(config).await?;

    let mut report = ExportReport { out_dir: out.to_path_buf(), ..Default::default() };

    let namespaces = configured_hosted_namespaces(config);
    for org in namespaces {
        let org_storage = if org.is_empty() { storage.clone() } else { storage.for_hosted(&org) };
        let base_out = if org.is_empty() { out.to_path_buf() } else { out.join(&org) };

        let package_names = org_storage
            .hosted_package_names()
            .await
            .map_err(|err| miette::miette!("failed to list hosted packages: {err}"))?;

        for pkg_name in package_names {
            export_package(&org_storage, &pkg_name, &base_out, &mut report).await?;
        }
    }

    if include_auth {
        export_auth_files(config, out).await?;
    }

    Ok(report)
}

/// The configured hosted store, once its layout is one this pnpr reads.
pub(crate) async fn open_hosted_storage(config: &Config) -> miette::Result<Storage> {
    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )
    .map_err(|err| miette::miette!("failed to initialize storage: {err}"))?;
    storage
        .check_hosted_layout()
        .await
        .map_err(|err| miette::miette!("{err}"))?;
    Ok(storage)
}

pub(crate) fn configured_hosted_namespaces(config: &Config) -> Vec<String> {
    let mut orgs = Vec::new();
    for hosted in config.routing.hosted.values() {
        if !orgs.contains(&hosted.org) {
            orgs.push(hosted.org.clone());
        }
    }
    if orgs.is_empty() {
        orgs.push(String::new());
    }
    orgs
}

async fn export_package(
    storage: &Storage,
    pkg_name: &str,
    out: &Path,
    report: &mut ExportReport,
) -> miette::Result<()> {
    let Ok(canonical) = CanonicalPackageName::parse(pkg_name, Ecosystem::Npm) else {
        return Ok(());
    };

    let doc_bytes = storage
        .read_hosted_document(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to read document for '{pkg_name}': {err}"))?;

    let Some(bytes) = doc_bytes else {
        return Ok(());
    };

    let pkg_dir = out.join(pkg_name);
    fs::create_dir_all(&pkg_dir).await.into_diagnostic()?;
    pnpr_storage::write_atomic(&pkg_dir.join("package.json"), &bytes).await.into_diagnostic()?;
    report.packages_exported += 1;

    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|err| miette::miette!("invalid JSON in '{pkg_name}': {err}"))?;

    if let Some(versions) = doc.get("versions").and_then(Value::as_object) {
        for (ver, ver_val) in versions {
            report.versions_exported += 1;
            export_version_blobs(storage, &canonical, ver, ver_val, &pkg_dir, report).await?;
        }
    }
    Ok(())
}

async fn export_version_blobs(
    storage: &Storage,
    canonical: &CanonicalPackageName,
    ver: &str,
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
    if dist
        .get("attestations")
        .and_then(|a| a.get("url"))
        .is_some()
    {
        let sigstore_name = canonical.sigstore_name_for_version(ver);
        export_single_blob(storage, canonical, &sigstore_name, pkg_dir, report).await?;
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
    let Some(safe_name) = safe_filename(filename) else {
        return Ok(());
    };
    let dest_file = pkg_dir.join(safe_name);
    if dest_file.exists() {
        return Ok(());
    }

    let opened = storage
        .open_hosted_blob(canonical, safe_name)
        .await
        .map_err(|err| miette::miette!("failed to read blob '{safe_name}': {err}"))?;

    if let Some((body, _)) = opened {
        let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
        pnpr_storage::write_atomic(&dest_file, &bytes).await.into_diagnostic()?;
        report.blobs_exported += 1;
        report.bytes_exported += bytes.len() as u64;
    }
    Ok(())
}

async fn export_auth_files(config: &Config, out: &Path) -> miette::Result<()> {
    if let Some(ht_path) = config.identity.auth.htpasswd.file.as_deref().filter(|p| p.exists()) {
        let bytes = fs::read(ht_path).await.into_diagnostic()?;
        pnpr_auth::write_atomic(&out.join("htpasswd"), &bytes).into_diagnostic()?;
    }
    if let Some(tok_path) = config.identity.auth.tokens.file.as_deref().filter(|p| p.exists()) {
        let bytes = fs::read(tok_path).await.into_diagnostic()?;
        pnpr_auth::write_atomic(&out.join("tokens.db"), &bytes).into_diagnostic()?;
    }
    Ok(())
}

fn safe_filename(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    let path = Path::new(name);
    if path.file_name().and_then(|f| f.to_str()) != Some(name) {
        return None;
    }
    Some(name)
}

fn filename_from_url(url: &str) -> Option<&str> {
    let segment = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()?;
    safe_filename(segment)
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
