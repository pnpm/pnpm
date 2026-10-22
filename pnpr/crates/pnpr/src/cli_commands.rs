pub mod args;
pub use args::{FeatureArgs, OsvArgs, ServerArgs, parse_cli_interval};

use crate::{
    Config, RegistryError,
    cache_maintenance::{CacheGcEffect, CacheGcScope},
};
use clap::Subcommand;
use pnpm_diagnostics::{print, println};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the registry server (default when no subcommand is specified).
    Serve(ServerArgs),
    /// Collect old, unreferenced image blobs. Stop all registry writers first.
    OciGc {
        /// Concrete hosted OCI registry from the config.
        #[arg(long)]
        registry: String,
        /// Report reclaimable blobs without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Minimum blob age to reclaim (e.g. "1d", "86400s", or bare seconds).
        #[arg(long, default_value = "1d", alias = "min-age-secs", value_parser = parse_cli_interval)]
        min_age: Duration,
    },
    /// Reclaim cached upstream tarballs and packuments.
    CacheGc {
        /// Report reclaimable entries without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Maximum total cache size to retain (e.g. "10GB", "500MB", or raw bytes).
        #[arg(long)]
        max_size: Option<String>,
        /// Maximum file age (e.g. "30d", "1h", or bare seconds). Entries older than this are deleted.
        #[arg(long, alias = "max-age-secs", value_parser = parse_cli_interval)]
        max_age: Option<Duration>,
        /// Only prune orphaned upstream namespaces without evicting active cache files.
        #[arg(long)]
        orphans_only: bool,
    },
    /// Validate configuration and report diagnostics.
    #[command(name = "config-check", alias = "check-config", alias = "check")]
    ConfigCheck,
    /// Configuration management commands.
    Config {
        #[command(subcommand)]
        action: ConfigSubcommand,
    },
    /// Verify every document against its blobs (tarballs, attestations).
    Verify {
        /// Skip cryptographic hash verification (SHA-512 / SHA-1) of stored blobs against packument integrity.
        #[arg(long, alias = "no-check-hashes")]
        skip_hashes: bool,
        /// Remove orphaned blobs from storage.
        #[arg(long)]
        prune_orphans: bool,
        /// Minimum age before an unreferenced blob is considered for pruning (e.g. "1d", "86400s", or bare seconds).
        #[arg(long, default_value = "1d", alias = "min-age-secs", value_parser = parse_cli_interval)]
        min_age: Duration,
    },
    /// Export hosted packages and blobs to a directory.
    Export {
        /// Target directory to export packages into.
        #[arg(long)]
        out: PathBuf,
        /// Include auth files (htpasswd, tokens.db) in export.
        #[arg(long)]
        include_auth: bool,
    },
    /// Migrate hosted packages and blobs from local disk storage to S3.
    MigrateStorage {
        /// Source local storage directory. Defaults to config storage directory.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Destination S3 bucket name. Defaults to config S3 bucket.
        #[arg(long)]
        to_bucket: Option<String>,
        /// Destination S3 region. Defaults to config S3 region.
        #[arg(long)]
        to_region: Option<String>,
        /// Destination S3 endpoint URL. Defaults to config S3 endpoint.
        #[arg(long)]
        to_endpoint: Option<String>,
        /// Destination S3 prefix. Defaults to config S3 prefix.
        #[arg(long)]
        to_prefix: Option<String>,
        /// Report what would be migrated without writing to S3.
        #[arg(long)]
        dry_run: bool,
        /// Verify uploaded blobs in target store after writing.
        #[arg(long)]
        verify: bool,
    },
    /// Convert the hosted store to the current storage layout. Back the store
    /// up and stop every pnpr process that uses it first.
    UpgradeStorage {
        /// Report what would change without changing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Import users from an existing Verdaccio or Apache htpasswd file.
    ImportHtpasswd {
        /// Path to source htpasswd file.
        #[arg(long)]
        source: PathBuf,
        /// Path to target htpasswd file. Defaults to config htpasswd file.
        #[arg(long)]
        target: Option<PathBuf>,
        /// Report what would be imported without writing to target file.
        #[arg(long)]
        dry_run: bool,
        /// Overwrite conflicting credentials for existing users in destination file.
        #[arg(long)]
        overwrite: bool,
    },
    /// Add or update a user account in the configured auth backend. With an
    /// htpasswd file, run it while the server is stopped.
    AddUser {
        /// Username of the account to create or update.
        username: String,
        #[arg(skip)]
        password: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Validate configuration and report diagnostics.
    Check,
}

pub async fn run_command(config: &Config, command: Command) -> miette::Result<()> {
    match command {
        Command::Serve(..) => Err(miette::miette!(
            "`serve` is not a maintenance command; start the server with `pnpr::serve`"
        )),
        Command::OciGc { .. } => run_oci_gc_command(config, command).await,
        Command::CacheGc { .. } => run_cache_gc_command(config, command).await,
        Command::ConfigCheck | Command::Config { .. } => run_config_check(),
        Command::Verify { .. } => run_verify_command(config, command).await,
        Command::Export { .. } => run_export_command(config, command).await,
        Command::MigrateStorage { .. } => run_migrate_command(config, command).await,
        Command::UpgradeStorage { dry_run } => {
            let report = crate::upgrade_storage::upgrade_storage(config, dry_run).await?;
            print!("{}", crate::upgrade_storage::format_upgrade_report(&report));
            Ok(())
        }
        Command::ImportHtpasswd { .. } => run_import_command(config, command),
        Command::AddUser { username, password } => {
            run_add_user_command(config, username, password).await
        }
    }
}

async fn run_oci_gc_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::OciGc { registry, dry_run, min_age } = command else {
        return Ok(());
    };
    run_oci_gc(config, registry, dry_run, min_age.as_secs()).await
}

async fn run_cache_gc_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::CacheGc {
        dry_run,
        max_size,
        max_age,
        orphans_only,
    } = command
    else {
        return Ok(());
    };
    let effect = if dry_run { CacheGcEffect::DryRun } else { CacheGcEffect::Apply };
    let scope = if orphans_only { CacheGcScope::OrphansOnly } else { CacheGcScope::Full };
    run_cache_gc(config, effect, max_size, max_age.map(|age| age.as_secs()), scope).await
}

async fn run_verify_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::Verify { skip_hashes, prune_orphans, min_age } = command else {
        return Ok(());
    };
    run_verify(config, !skip_hashes, prune_orphans, min_age.as_secs()).await
}

async fn run_export_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::Export { out, include_auth } = command else {
        return Ok(());
    };
    run_export(config, out, include_auth).await
}

async fn run_migrate_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::MigrateStorage {
        from,
        to_bucket,
        to_region,
        to_endpoint,
        to_prefix,
        dry_run,
        verify,
    } = command
    else {
        return Ok(());
    };
    let options = crate::migrate_storage::MigrateStorageOptions {
        from,
        to_bucket,
        to_region,
        to_endpoint,
        to_prefix,
        dry_run,
        verify,
    };
    run_migrate_storage(config, options).await
}

fn run_import_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::ImportHtpasswd { source, target, dry_run, overwrite } = command else {
        return Ok(());
    };
    run_import_htpasswd(config, source, target, dry_run, overwrite)
}

fn run_config_check() -> miette::Result<()> {
    let report = crate::config_check::check_config(None)?;
    println!("{}", crate::config_check::format_report(&report));
    Ok(())
}

async fn run_migrate_storage(
    config: &Config,
    options: crate::migrate_storage::MigrateStorageOptions,
) -> miette::Result<()> {
    let report = crate::migrate_storage::migrate_local_to_s3(config, &options).await?;
    print!("{}", crate::migrate_storage::format_migrate_report(&report));
    Ok(())
}

async fn run_verify(
    config: &Config,
    check_hashes: bool,
    prune_orphans: bool,
    min_age_secs: u64,
) -> miette::Result<()> {
    let options = crate::verify::VerifyOptions { check_hashes, prune_orphans, min_age_secs };
    let report = crate::verify::verify_storage(config, &options).await?;
    print!("{}", crate::verify::format_verify_report(&report));
    if !report.missing_blobs.is_empty() || !report.corrupted_blobs.is_empty() {
        let missing_count = report.missing_blobs.len();
        let corrupted_count = report.corrupted_blobs.len();
        return Err(miette::miette!(
            "storage verification failed: {missing_count} missing, {corrupted_count} corrupted blobs",
        ));
    }
    Ok(())
}

async fn run_export(config: &Config, out: PathBuf, include_auth: bool) -> miette::Result<()> {
    let report = crate::export::export_storage(config, &out, include_auth).await?;
    print!("{}", crate::export::format_export_report(&report));
    Ok(())
}

fn run_import_htpasswd(
    config: &Config,
    source: PathBuf,
    target: Option<PathBuf>,
    dry_run: bool,
    overwrite: bool,
) -> miette::Result<()> {
    let target = target.or_else(|| config.identity.auth.htpasswd.file.clone());
    let Some(target) = target else {
        return Err(miette::miette!(
            "no target htpasswd file specified and none configured in identity.auth.htpasswd.file; pass --target <path>"
        ));
    };
    let options = crate::htpasswd_import::HtpasswdImportOptions {
        source,
        target: Some(target),
        dry_run,
        overwrite,
    };
    let report = crate::htpasswd_import::import_htpasswd(&options)?;
    print!("{}", crate::htpasswd_import::format_import_report(&report));
    Ok(())
}

async fn run_oci_gc(
    config: &Config,
    registry: String,
    dry_run: bool,
    min_age_secs: u64,
) -> miette::Result<()> {
    crate::recover_publish_journal(config).await.map_err(|err| redacted_report(&err))?;
    let (blobs, bytes) = crate::oci_maintenance::collect_oci_blobs(
        config,
        &registry,
        Duration::from_secs(min_age_secs),
        dry_run,
    )
    .await
    .map_err(|err| redacted_report(&err))?;
    tracing::info!(blobs, bytes, dry_run, "OCI collection completed");
    Ok(())
}

async fn run_cache_gc(
    config: &Config,
    effect: CacheGcEffect,
    max_size: Option<String>,
    max_age_secs: Option<u64>,
    scope: CacheGcScope,
) -> miette::Result<()> {
    let max_size = if let Some(raw) = max_size.as_deref() {
        Some(pnpr_config::parse_byte_size(raw).ok_or_else(|| {
            miette::miette!(
                "invalid --max-size {raw:?}; expected byte size like '10GB', '500MB', or raw bytes"
            )
        })?)
    } else {
        None
    };
    let options = crate::cache_maintenance::CacheGcOptions {
        effect,
        max_size,
        max_age: max_age_secs.map(Duration::from_secs),
        scope,
    };
    let report = crate::cache_maintenance::collect_cache(config, &options).await
        .map_err(|err| redacted_report(&err))?;
    tracing::info!(
        namespaces = report.namespaces_removed,
        files = report.files_evicted,
        bytes = report.bytes_reclaimed,
        effect = ?effect,
        "cache collection completed",
    );
    Ok(())
}

fn redacted_report(err: &RegistryError) -> miette::Report {
    let message = err.log_message();
    miette::miette!("{message}")
}

fn read_password() -> miette::Result<String> {
    use std::io::IsTerminal;
    let trimmed = if std::io::stdin().is_terminal() {
        dialoguer::Password::new()
            .with_prompt("Enter password")
            .allow_empty_password(true)
            .interact()
            .map_err(|err| miette::miette!("could not read password from terminal: {err}"))?
    } else {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|err| miette::miette!("could not read password from stdin: {err}"))?;
        line.trim_end_matches(['\r', '\n'])
            .to_string()
    };
    if trimmed.is_empty() {
        return Err(miette::miette!("password cannot be empty"));
    }
    Ok(trimmed)
}

async fn run_add_user_command(
    config: &Config,
    username: String,
    password: Option<String>,
) -> miette::Result<()> {
    if matches!(config.identity.backend, pnpr_config::BackendConfig::Local)
        && config.identity.auth.htpasswd.file.is_none()
    {
        return Err(miette::miette!(
            "standalone add-user requires a persistent auth backend (configure auth.htpasswd.file or a database backend)"
        ));
    }
    let password = match password {
        Some(password) => password,
        None => read_password()?,
    };
    if password.is_empty() {
        return Err(miette::miette!("password cannot be empty"));
    }
    if password.len() > pnpr_auth::MAX_PASSWORD_BYTES {
        return Err(miette::miette!(
            "password cannot exceed {} bytes",
            pnpr_auth::MAX_PASSWORD_BYTES,
        ));
    }
    let auth = pnpr_auth::AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .map_err(|err| miette::miette!("could not connect to auth backend: {err}"))?;
    match auth
        .put_user(&username, &password)
        .await
        .map_err(|err| miette::miette!("{err}"))?
    {
        Some(pnpr_auth::UserPut::Created) => println!("User '{username}' created successfully."),
        Some(pnpr_auth::UserPut::PasswordChanged) => {
            println!("Password for user '{username}' updated successfully.");
        }
        None => return Err(miette::miette!("failed to create or update user '{username}'")),
    }
    Ok(())
}
