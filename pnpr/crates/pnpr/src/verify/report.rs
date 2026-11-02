//! What `pnpr verify` found, and how it is printed.

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub packages_checked: usize,
    pub versions_checked: usize,
    pub blobs: BlobTotals,
    pub missing_blobs: Vec<String>,
    pub corrupted_blobs: Vec<String>,
    pub orphaned_blobs: Vec<String>,
    pub prune_failures: Vec<String>,
    /// Versions whose stored metadata disagrees with the `package.json` in
    /// their stored archive under the manifest interpretation profile. A
    /// version published before publish-time verification can; whether it is
    /// then served from its archive, blocked, or left as it is, is the
    /// operator's decision.
    pub manifest_disagreements: Vec<String>,
}

/// How many stored blobs were read, and their total size.
#[derive(Debug, Default)]
pub struct BlobTotals {
    pub checked: usize,
    pub bytes: u64,
}

#[must_use]
pub fn format_verify_report(report: &VerifyReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("Storage verification completed:\n");
    let _ = writeln!(out, "  Packages checked: {}", report.packages_checked);
    let _ = writeln!(out, "  Versions checked: {}", report.versions_checked);
    let _ =
        writeln!(out, "  Blobs verified: {} ({} bytes)", report.blobs.checked, report.blobs.bytes);
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
    let _ = writeln!(out, "  Manifest disagreements: {}", report.manifest_disagreements.len());
    for disagreement in &report.manifest_disagreements {
        let _ = writeln!(out, "    - MANIFEST: {disagreement}");
    }
    if !report.prune_failures.is_empty() {
        let _ = writeln!(out, "  Prune failures: {}", report.prune_failures.len());
        for failure in &report.prune_failures {
            let _ = writeln!(out, "    - PRUNE FAILED: {failure}");
        }
    }
    out
}
