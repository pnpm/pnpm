use miette::IntoDiagnostic as _;
use pnpr_auth::{is_bcrypt_hash, is_sha_hash, parse_htpasswd_line};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone)]
pub struct HtpasswdImportOptions {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub dry_run: bool,
    pub overwrite: bool,
}

#[derive(Debug)]
pub struct HtpasswdImportReport {
    pub total_read: usize,
    pub bcrypt_count: usize,
    pub sha_count: usize,
    pub merged: usize,
    pub target_path: PathBuf,
    pub dry_run: bool,
    pub collisions: Vec<String>,
}

pub fn import_htpasswd(options: &HtpasswdImportOptions) -> miette::Result<HtpasswdImportReport> {
    if !options.source.exists() {
        let display_source = options.source.display();
        return Err(miette::miette!("source htpasswd file '{display_source}' does not exist"));
    }

    let Some(target_path) = options.target.clone() else {
        return Err(miette::miette!("no target htpasswd file specified"));
    };

    let source_content = std::fs::read_to_string(&options.source)
        .into_diagnostic()
        .map_err(|err| miette::miette!("failed to read source file: {err}"))?;

    let (source_users, total_read, bcrypt_count, sha_count) =
        parse_source_entries(&source_content)?;
    let (merged, collisions) = pnpr_auth::UserStore::import_password_hashes(
        &target_path,
        source_users,
        pnpr_auth::PasswordImport {
            collisions: if options.overwrite {
                pnpr_auth::PasswordCollisions::Overwrite
            } else {
                pnpr_auth::PasswordCollisions::Keep
            },
            mode: if options.dry_run {
                pnpr_auth::ImportMode::DryRun
            } else {
                pnpr_auth::ImportMode::Persist
            },
        },
    )
    .into_diagnostic()?;

    Ok(HtpasswdImportReport {
        total_read,
        bcrypt_count,
        sha_count,
        merged,
        target_path,
        dry_run: options.dry_run,
        collisions,
    })
}

fn parse_source_entries(
    raw: &str,
) -> miette::Result<(HashMap<String, String>, usize, usize, usize)> {
    let mut users = HashMap::new();
    let mut total_read = 0;
    let mut bcrypt_count = 0;
    let mut sha_count = 0;

    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches(['\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (user, hash) = parse_htpasswd_line(line)
            .map_err(|err| miette::miette!("line {}: {err}", line_no + 1))?;

        total_read += 1;
        if is_bcrypt_hash(hash) {
            bcrypt_count += 1;
        } else if is_sha_hash(hash) {
            sha_count += 1;
        }
        users.insert(user.to_string(), hash.to_string());
    }

    Ok((users, total_read, bcrypt_count, sha_count))
}

#[must_use]
pub fn format_import_report(report: &HtpasswdImportReport) -> String {
    use std::fmt::Write as _;
    let action = if report.dry_run { "would be imported" } else { "imported" };
    let display_target = report.target_path.display();
    let mut out = format!(
        "htpasswd import completed: {} users {} ({} bcrypt, {} {{SHA}}) into {display_target}\n",
        report.merged, action, report.bcrypt_count, report.sha_count,
    );
    if !report.collisions.is_empty() {
        let count = report.collisions.len();
        let _ = writeln!(out, "  warning: {count} conflicting credentials not overwritten:");
        for collision in &report.collisions {
            let _ = writeln!(out, "    - {collision}");
        }
    }
    out
}
