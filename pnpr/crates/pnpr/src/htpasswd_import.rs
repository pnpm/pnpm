use miette::IntoDiagnostic as _;
use pnpr_auth::{is_bcrypt_hash, is_sha_hash, parse_htpasswd, parse_htpasswd_line};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

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

    let (source_users, bcrypt_count, sha_count) = parse_source_entries(&source_content)?;
    let mut target_users = load_existing_target(&target_path)?;

    let (merged, collisions) = merge_users(&mut target_users, source_users, options.overwrite);

    if !options.dry_run {
        save_target_users(&target_path, &target_users)?;
    }

    Ok(HtpasswdImportReport {
        total_read: bcrypt_count + sha_count,
        bcrypt_count,
        sha_count,
        merged,
        target_path,
        dry_run: options.dry_run,
        collisions,
    })
}

fn parse_source_entries(raw: &str) -> miette::Result<(HashMap<String, String>, usize, usize)> {
    let mut users = HashMap::new();
    let mut bcrypt_count = 0;
    let mut sha_count = 0;

    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches(['\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (user, hash) = parse_htpasswd_line(line)
            .map_err(|err| miette::miette!("line {}: {err}", line_no + 1))?;

        if is_bcrypt_hash(hash) {
            bcrypt_count += 1;
        } else if is_sha_hash(hash) {
            sha_count += 1;
        }
        users.insert(user.to_string(), hash.to_string());
    }

    Ok((users, bcrypt_count, sha_count))
}

fn load_existing_target(target_path: &Path) -> miette::Result<HashMap<String, String>> {
    if !target_path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(target_path)
        .into_diagnostic()
        .map_err(|err| miette::miette!("failed to read target file: {err}"))?;
    parse_htpasswd(&content).map_err(|err| miette::miette!("failed to parse target file: {err}"))
}

fn merge_users(
    target: &mut HashMap<String, String>,
    source: HashMap<String, String>,
    overwrite: bool,
) -> (usize, Vec<String>) {
    let mut merged = 0;
    let mut collisions = Vec::new();
    for (user, source_hash) in source {
        match target.get(&user) {
            Some(existing) if existing == &source_hash => {}
            Some(existing) if !overwrite => {
                let d_kind = hash_kind(existing);
                let s_kind = hash_kind(&source_hash);
                collisions.push(format!("{user} (destination: {d_kind}, source: {s_kind})"));
            }
            _ => {
                target.insert(user, source_hash);
                merged += 1;
            }
        }
    }
    (merged, collisions)
}

fn hash_kind(hash: &str) -> &'static str {
    if is_bcrypt_hash(hash) {
        "bcrypt"
    } else if is_sha_hash(hash) {
        "{SHA}"
    } else {
        "unrecognized"
    }
}

fn save_target_users(path: &Path, users: &HashMap<String, String>) -> miette::Result<()> {
    let mut entries: Vec<(&String, &String)> = users.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));

    let mut out = String::new();
    for (user, hash) in entries {
        out.push_str(user);
        out.push(':');
        out.push_str(hash);
        out.push('\n');
    }

    pnpr_auth::write_atomic(path, out.as_bytes()).into_diagnostic()?;
    Ok(())
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
        let _ = writeln!(
            out,
            "  warning: {count} conflicting credentials not overwritten (pass --overwrite to replace):",
        );
        for collision in &report.collisions {
            let _ = writeln!(out, "    - {collision}");
        }
    }
    out
}
