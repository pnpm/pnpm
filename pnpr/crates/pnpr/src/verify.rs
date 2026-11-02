pub use report::{BlobTotals, VerifyReport, format_verify_report};

mod manifests;
mod report;

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
        let org_storage = storage.for_hosted(&org, pnpr_registry::Ecosystem::Npm);
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

    // A hosted tar registry stores the version's tar in place of its `.tgz`.
    let tar = canonical.tar_name_for_version(ver);
    ctx.referenced_blobs.insert(format!("{pkg_name}/{filename}"));
    ctx.referenced_blobs.insert(format!("{pkg_name}/{tar}"));

    if let Some(bytes) = read_blob(ctx, canonical, filename).await? {
        if ctx.options.check_hashes {
            check_hash_mismatch(pkg_name, ver, filename, &bytes, dist, ctx.report);
            let tar = flate2::read::MultiGzDecoder::new(bytes.as_ref());
            ctx.report.manifest_disagreements.extend(manifests::manifest_disagreement(
                pkg_name, ver, ver_val, tar,
            ));
        }
    } else if !verify_stored_tar(ctx, canonical, ver, ver_val).await? {
        ctx.report.missing_blobs.push(format!("{pkg_name}@{ver}: {filename}"));
    }
    Ok(())
}

/// Check the tar a hosted tar registry stores for `ver` in place of its
/// `.tgz`. `false` when there is none.
async fn verify_stored_tar(
    ctx: &mut VerifyContext<'_>,
    canonical: &CanonicalPackageName,
    ver: &str,
    ver_val: &Value,
) -> miette::Result<bool> {
    let tar = canonical.tar_name_for_version(ver);
    let Some(bytes) = read_blob(ctx, canonical, &tar).await? else { return Ok(false) };
    if !ctx.options.check_hashes {
        return Ok(true);
    }
    let pkg_name = canonical.as_str();
    let dist = ver_val.get("dist").and_then(Value::as_object);
    let dist = dist.cloned().unwrap_or_default();
    if let Some(problem) = stored_tar_problem(ctx.storage, canonical, &bytes, &dist).await? {
        ctx.report.corrupted_blobs.push(format!("{pkg_name}@{ver}: {tar} ({problem})"));
    } else if let Ok(decoded) = zstd::stream::read::Decoder::new(bytes.as_ref()) {
        ctx.report.manifest_disagreements.extend(manifests::manifest_disagreement(
            pkg_name, ver, ver_val, decoded,
        ));
    }
    Ok(true)
}

/// What is wrong with a stored tar, checked against the tar integrity
/// recorded at publish: `dist.integrity` names the `.tgz`, which was not kept.
async fn stored_tar_problem(
    storage: &Storage,
    canonical: &CanonicalPackageName,
    stored: &[u8],
    dist: &serde_json::Map<String, Value>,
) -> miette::Result<Option<String>> {
    let table = storage
        .read_tar_integrities(canonical)
        .await
        .map_err(|err| {
            miette::miette!("failed to read tar integrities for '{}': {err}", canonical.as_str())
        })?;
    let expected = dist
        .get("integrity")
        .and_then(Value::as_str)
        .and_then(|integrity| integrity.parse::<ssri::Integrity>().ok())
        .and_then(|integrity| table.get(&integrity.to_string()));
    let Some(expected) = expected else {
        return Ok(Some("no recorded tar integrity".to_string()));
    };
    let Ok(tar) = zstd::decode_all(stored) else {
        return Ok(Some("not a zstd stream".to_string()));
    };
    Ok((!verify_blob_integrity(&tar, &expected.tar_integrity)).then(|| {
        format!("tar integrity mismatch: {}", expected.tar_integrity)
    }))
}

/// The bytes of the hosted blob `filename`, counted as checked.
async fn read_blob(
    ctx: &mut VerifyContext<'_>,
    canonical: &CanonicalPackageName,
    filename: &str,
) -> miette::Result<Option<axum::body::Bytes>> {
    let opened = ctx.storage
        .open_hosted_blob(canonical, filename)
        .await
        .map_err(|err| miette::miette!("error opening blob for '{}': {err}", canonical.as_str()))?;
    let Some((body, _)) = opened else { return Ok(None) };
    let bytes = to_bytes(body, usize::MAX).await.into_diagnostic()?;
    ctx.report.blobs.checked += 1;
    ctx.report.blobs.bytes += bytes.len() as u64;
    Ok(Some(bytes))
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
