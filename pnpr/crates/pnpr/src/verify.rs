use axum::body::to_bytes;
use miette::IntoDiagnostic as _;
use pnpr_config::Config;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{Storage, metadata::BlobState};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub check_hashes: bool,
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
}

struct VerifyContext<'a> {
    storage: &'a Storage,
    options: &'a VerifyOptions,
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
        let package_names = org_storage
            .hosted_package_names()
            .await
            .map_err(|err| miette::miette!("failed to list hosted packages: {err}"))?;
        let mut ctx = VerifyContext { storage: &org_storage, options, report: &mut report };
        for pkg_name in package_names {
            verify_package(&mut ctx, &pkg_name).await?;
        }
        for ecosystem in pnpr_registry::Ecosystem::all() {
            check_orphaned_archives(&storage.for_hosted(&org, ecosystem), &mut report).await?;
        }
    }

    Ok(report)
}

async fn verify_package(ctx: &mut VerifyContext<'_>, pkg_name: &str) -> miette::Result<()> {
    let Ok(canonical) = CanonicalPackageName::parse(pkg_name, pnpr_registry::Ecosystem::Npm) else {
        return Ok(());
    };

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
    if let Some(bytes) = read_blob(ctx, canonical, filename).await? {
        if ctx.options.check_hashes {
            check_hash_mismatch(pkg_name, ver, filename, &bytes, dist, ctx.report);
        }
    } else if !verify_stored_tar(ctx, canonical, ver, dist).await? {
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
    dist: &serde_json::Map<String, Value>,
) -> miette::Result<bool> {
    let tar = canonical.tar_name_for_version(ver);
    let Some(bytes) = read_blob(ctx, canonical, &tar).await? else { return Ok(false) };
    if ctx.options.check_hashes
        && let Some(problem) = stored_tar_problem(ctx.storage, canonical, &bytes, dist).await?
    {
        let pkg_name = canonical.as_str();
        ctx.report.corrupted_blobs.push(format!("{pkg_name}@{ver}: {tar} ({problem})"));
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
    ctx.report.blobs_checked += 1;
    ctx.report.bytes_verified += bytes.len() as u64;
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

    let opened = ctx.storage
        .open_hosted_blob(canonical, &filename)
        .await
        .map_err(|err| miette::miette!("error opening attestation for '{pkg_name}': {err}"))?;

    if opened.is_none() {
        ctx.report.missing_blobs.push(format!("{pkg_name}@{ver} attestation: {filename}"));
    }
    Ok(())
}

/// Report every archive `storage` holds that is not the live incarnation of
/// a blob some file maps: what a refused publish, an interrupted reclaim, or
/// an unpublish left. `pnpr gc` removes them, fenced against commits that
/// land meanwhile.
async fn check_orphaned_archives(
    storage: &Storage,
    report: &mut VerifyReport,
) -> miette::Result<()> {
    let listed = |err| miette::miette!("failed to list archives: {err}");
    let stored = storage.stored_incarnations().await.map_err(listed)?;
    if stored.is_empty() {
        return Ok(());
    }
    let ids = stored
        .iter()
        .map(|(blob, _)| blob.id.clone())
        .collect();
    let rows = storage.blob_rows(ids).await.map_err(listed)?;
    let unmapped = storage.unmapped_blobs().await.map_err(listed)?;
    for (blob, _) in stored {
        let mapped_live = rows
            .get(&blob.id)
            .is_some_and(|row| row.state == BlobState::Live && row.incarnation == blob.incarnation)
            && !unmapped.contains_key(&blob.id);
        if !mapped_live {
            report.orphaned_blobs.push(blob.key());
        }
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
    out
}
