use axum::body::to_bytes;
use futures_util::StreamExt as _;
use miette::IntoDiagnostic as _;
use pnpr_config::Config;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::Storage;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub check_hashes: bool,
    pub prune_orphans: bool,
    pub min_age_secs: u64,
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub packages_checked: usize,
    pub versions_checked: usize,
    pub blobs_checked: usize,
    pub bytes_verified: u64,
    pub missing_blobs: Vec<String>,
    pub corrupted_blobs: Vec<String>,
    pub orphaned_blobs: Vec<String>,
    pub prune_failures: Vec<String>,
}

struct VerifyContext<'a> {
    storage: &'a Storage,
    options: &'a VerifyOptions,
    referenced_blobs: &'a mut HashSet<String>,
    verified_npm_packages: &'a mut HashSet<String>,
    report: &'a mut VerifyReport,
}

pub async fn verify_storage(
    config: &Config,
    options: &VerifyOptions,
) -> miette::Result<VerifyReport> {
    let storage = crate::export::open_hosted_storage(config).await?;

    let mut report = VerifyReport::default();
    let namespaces = crate::export::configured_hosted_namespaces(config);

    for org in namespaces {
        let org_storage = if org.is_empty() { storage.clone() } else { storage.for_hosted(&org) };
        let mut referenced_blobs = HashSet::new();
        let mut verified_npm_packages = HashSet::new();

        let package_names = org_storage
            .hosted_package_names()
            .await
            .map_err(|err| miette::miette!("failed to list hosted packages: {err}"))?;

        {
            let mut ctx = VerifyContext {
                storage: &org_storage,
                options,
                referenced_blobs: &mut referenced_blobs,
                verified_npm_packages: &mut verified_npm_packages,
                report: &mut report,
            };
            for pkg_name in package_names {
                verify_package(&mut ctx, &pkg_name).await?;
            }
        }

        check_orphaned_blobs(
            &org_storage,
            &referenced_blobs,
            &verified_npm_packages,
            options,
            &mut report,
        )
        .await?;
    }

    Ok(report)
}

async fn verify_package(ctx: &mut VerifyContext<'_>, pkg_name: &str) -> miette::Result<()> {
    let Ok(canonical) = CanonicalPackageName::parse(pkg_name, pnpr_registry::Ecosystem::Npm) else {
        return Ok(());
    };
    ctx.verified_npm_packages.insert(pkg_name.to_string());

    let doc_bytes = ctx.storage
        .read_hosted_document(&canonical)
        .await
        .map_err(|err| miette::miette!("failed to read document for '{pkg_name}': {err}"))?;

    let Some(bytes) = doc_bytes else {
        return Ok(());
    };

    ctx.report.packages_checked += 1;
    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|err| miette::miette!("invalid JSON packument for '{pkg_name}': {err}"))?;

    if let Some(versions) = doc.get("versions").and_then(Value::as_object) {
        for (ver, ver_val) in versions {
            ctx.report.versions_checked += 1;
            verify_version_tarball(ctx, &canonical, pkg_name, ver, ver_val).await?;
            verify_version_attestation(ctx, &canonical, pkg_name, ver, ver_val).await?;
        }
    }
    Ok(())
}

async fn verify_version_tarball(
    ctx: &mut VerifyContext<'_>,
    canonical: &CanonicalPackageName,
    pkg_name: &str,
    ver: &str,
    ver_val: &Value,
) -> miette::Result<()> {
    let Some(dist) = ver_val.get("dist").and_then(Value::as_object) else {
        return Ok(());
    };

    let Some(filename) = dist
        .get("tarball")
        .and_then(Value::as_str)
        .and_then(filename_from_url)
    else {
        return Ok(());
    };

    let blob_key = format!("{pkg_name}/{filename}");
    ctx.referenced_blobs.insert(blob_key);

    let opened = ctx.storage
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("error opening blob for '{pkg_name}': {err}"))?;

    match opened {
        None => {
            ctx.report.missing_blobs.push(format!("{pkg_name}@{ver}: {filename}"));
        }
        Some((body, _)) => {
            let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
            ctx.report.blobs_checked += 1;
            ctx.report.bytes_verified += bytes.len() as u64;

            if ctx.options.check_hashes {
                check_hash_mismatch(pkg_name, ver, filename, &bytes, dist, ctx.report);
            }
        }
    }
    Ok(())
}

fn check_hash_mismatch(
    pkg_name: &str,
    ver: &str,
    filename: &str,
    bytes: &[u8],
    dist: &serde_json::Map<String, Value>,
    report: &mut VerifyReport,
) {
    if let Some(integrity_str) = dist
        .get("integrity")
        .and_then(Value::as_str)
        .filter(|integrity| !verify_blob_integrity(bytes, integrity))
    {
        report.corrupted_blobs.push(format!(
            "{pkg_name}@{ver}: {filename} (integrity mismatch: {integrity_str})",
        ));
    } else if let Some(shasum_str) = dist
        .get("shasum")
        .and_then(Value::as_str)
        .filter(|shasum| !verify_blob_shasum(bytes, shasum))
    {
        report.corrupted_blobs.push(format!(
            "{pkg_name}@{ver}: {filename} (shasum mismatch: {shasum_str})",
        ));
    }
}

async fn verify_version_attestation(
    ctx: &mut VerifyContext<'_>,
    canonical: &CanonicalPackageName,
    pkg_name: &str,
    ver: &str,
    ver_val: &Value,
) -> miette::Result<()> {
    let Some(_) = ver_val
        .get("dist")
        .and_then(|d| d.get("attestations"))
        .and_then(|a| a.get("url"))
    else {
        return Ok(());
    };

    let filename = canonical.sigstore_name_for_version(ver);
    let blob_key = format!("{pkg_name}/{filename}");
    ctx.referenced_blobs.insert(blob_key);

    let opened = ctx.storage
        .open_hosted_blob(canonical, &filename)
        .await
        .map_err(|err| miette::miette!("error opening attestation for '{pkg_name}': {err}"))?;

    if opened.is_none() {
        ctx.report.missing_blobs.push(format!("{pkg_name}@{ver} attestation: {filename}"));
    }
    Ok(())
}

async fn check_orphaned_blobs(
    storage: &Storage,
    referenced: &HashSet<String>,
    verified_npm_packages: &HashSet<String>,
    options: &VerifyOptions,
    report: &mut VerifyReport,
) -> miette::Result<()> {
    let mut stream = storage.hosted_blob_files();
    while let Some(file_res) = stream.next().await {
        let file = file_res.map_err(|err| miette::miette!("failed to list blobs: {err}"))?;
        let Some(blob) = &file.blob else {
            continue;
        };
        let path = format!("{}/{blob}", file.package);
        if !verified_npm_packages.contains(&file.package) || referenced.contains(&path) {
            continue;
        }

        let is_young = file.modified
            .elapsed()
            .map_or(true, |elapsed| elapsed.as_secs() < options.min_age_secs);
        if options.prune_orphans && is_young {
            continue;
        }

        handle_orphan_blob(storage, &path, options.prune_orphans, report).await;
    }
    Ok(())
}

async fn handle_orphan_blob(storage: &Storage, path: &str, prune: bool, report: &mut VerifyReport) {
    if prune {
        if is_blob_referenced_in_fresh_packument(storage, path).await {
            return;
        }
        if let Err(err) = prune_orphan_blob(storage, path).await {
            report.prune_failures.push(format!("{path}: {err}"));
            report.orphaned_blobs.push(path.to_string());
            return;
        }
    }
    report.orphaned_blobs.push(path.to_string());
}

async fn is_blob_referenced_in_fresh_packument(storage: &Storage, path: &str) -> bool {
    let Some((pkg, file_name)) = path.rsplit_once('/') else {
        return false;
    };
    let Ok(canon) = CanonicalPackageName::parse(pkg, pnpr_registry::Ecosystem::Npm) else {
        return false;
    };
    let Ok(Some(doc_bytes)) = storage.read_hosted_document(&canon).await else {
        return false;
    };
    let Ok(packument) = serde_json::from_slice::<Value>(&doc_bytes) else {
        return false;
    };
    check_packument_for_blob(&packument, &canon, file_name)
}

fn check_packument_for_blob(
    packument: &Value,
    canon: &CanonicalPackageName,
    file_name: &str,
) -> bool {
    let Some(versions) = packument.get("versions").and_then(Value::as_object) else {
        return false;
    };
    for (ver, v) in versions {
        let tarball_match = v
            .get("dist")
            .and_then(|d| d.get("tarball"))
            .and_then(Value::as_str)
            .and_then(filename_from_url)
            .is_some_and(|name| name == file_name);
        let attestation_match = v
            .get("dist")
            .and_then(|d| d.get("attestations"))
            .and_then(|a| a.get("url"))
            .is_some()
            && canon.sigstore_name_for_version(ver) == file_name;
        if tarball_match || attestation_match {
            return true;
        }
    }
    false
}

async fn prune_orphan_blob(storage: &Storage, path: &str) -> Result<(), String> {
    let Some((pkg, file_name)) = path.rsplit_once('/') else {
        return Err("invalid blob path format".to_string());
    };
    let Ok(canon) = CanonicalPackageName::parse(pkg, pnpr_registry::Ecosystem::Npm) else {
        return Err(format!("cannot parse npm package name from path '{pkg}'"));
    };
    storage
        .remove_hosted_blob(&canon, file_name)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
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

fn filename_from_url(url: &str) -> Option<&str> {
    let segment = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()?;
    safe_filename(segment)
}

fn verify_blob_integrity(bytes: &[u8], integrity_str: &str) -> bool {
    integrity_str
        .parse::<ssri::Integrity>()
        .is_ok_and(|expected| expected.check(bytes).is_ok())
}

fn verify_blob_shasum(bytes: &[u8], expected_hex: &str) -> bool {
    ssri::Integrity::from_hex(expected_hex, ssri::Algorithm::Sha1)
        .is_ok_and(|expected| expected.check(bytes).is_ok())
}

#[must_use]
pub fn format_verify_report(report: &VerifyReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("Storage verification completed:\n");
    let _ = writeln!(out, "  Packages checked: {}", report.packages_checked);
    let _ = writeln!(out, "  Versions checked: {}", report.versions_checked);
    let _ = writeln!(
        out,
        "  Blobs verified: {} ({} bytes)",
        report.blobs_checked, report.bytes_verified,
    );
    let _ = writeln!(out, "  Missing blobs: {}", report.missing_blobs.len());
    for missing in &report.missing_blobs {
        let _ = writeln!(out, "    - MISSING: {missing}");
    }
    let _ = writeln!(out, "  Corrupted blobs: {}", report.corrupted_blobs.len());
    for corrupt in &report.corrupted_blobs {
        let _ = writeln!(out, "    - CORRUPTED: {corrupt}");
    }
    let _ = writeln!(out, "  Orphaned blobs: {}", report.orphaned_blobs.len());
    for orphan in &report.orphaned_blobs {
        let _ = writeln!(out, "    - ORPHANED: {orphan}");
    }
    if !report.prune_failures.is_empty() {
        let _ = writeln!(out, "  Prune failures: {}", report.prune_failures.len());
        for failure in &report.prune_failures {
            let _ = writeln!(out, "    - PRUNE FAILED: {failure}");
        }
    }
    out
}
