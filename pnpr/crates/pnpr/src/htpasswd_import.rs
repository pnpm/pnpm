use miette::IntoDiagnostic as _;
use pnpr_auth::{
    ImportMode::{Apply, Preview},
    PasswordCollisions::{Keep, Overwrite},
    is_bcrypt_hash, is_sha_hash, parse_htpasswd_line,
};
use pnpr_config::{BackendConfig, Config};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone)]
pub struct HtpasswdImportOptions {
    pub source: PathBuf,
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

/// Import the accounts of an htpasswd file into the `SQLite` metadata
/// database.
pub async fn import_htpasswd(
    config: &Config,
    options: &HtpasswdImportOptions,
) -> miette::Result<HtpasswdImportReport> {
    let BackendConfig::Sqlite(target_path) = &config.identity.backend else {
        return Err(miette::miette!(
            "htpasswd import writes to the SQLite metadata database; this config keeps accounts in a shared database"
        ));
    };
    let source_content = std::fs::read_to_string(&options.source)
        .into_diagnostic()
        .map_err(|err| {
            let source = options.source.display();
            miette::miette!("failed to read htpasswd file '{source}': {err}")
        })?;

    let (source_users, total_read, bcrypt_count, sha_count) =
        parse_source_entries(&source_content)?;
    let auth = open_accounts(target_path.clone(), config.identity.auth.users.max_users).await?;
    let imported = auth
        .import_password_hashes(
            source_users,
            pnpr_auth::PasswordImport {
                collisions: if options.overwrite { Overwrite } else { Keep },
                mode: if options.dry_run { Preview } else { Apply },
            },
        )
        .await
        .into_diagnostic()?;

    Ok(HtpasswdImportReport {
        total_read,
        bcrypt_count,
        sha_count,
        merged: imported.merged,
        target_path: target_path.clone(),
        dry_run: options.dry_run,
        collisions: imported.collisions,
    })
}

async fn open_accounts(
    path: PathBuf,
    max_users: pnpr_config::MaxUsers,
) -> miette::Result<pnpr_auth::SqliteAuth> {
    tokio::task::spawn_blocking(move || pnpr_auth::SqliteAuth::open(&path, max_users))
        .await
        .into_diagnostic()?
        .into_diagnostic()
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
