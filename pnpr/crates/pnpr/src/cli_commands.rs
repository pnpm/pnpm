use clap::Subcommand;
use pnpm_diagnostics::{print, println};
use pnpr::{Config, RegistryError};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Collect old, unreferenced image blobs. Stop all registry writers first.
    OciGc {
        /// Concrete hosted OCI registry from the config.
        #[arg(long)]
        registry: String,
        /// Report reclaimable blobs without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Minimum blob age to reclaim, in seconds.
        #[arg(long, default_value_t = 86400)]
        min_age_secs: u64,
    },
    /// Reclaim cached upstream tarballs and packuments.
    CacheGc {
        /// Report reclaimable entries without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Maximum total cache size to retain (e.g. "10GB", "500MB", or raw bytes).
        #[arg(long)]
        max_size: Option<String>,
        /// Maximum file age in seconds. Entries older than this are deleted.
        #[arg(long)]
        max_age_secs: Option<u64>,
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
        /// Check cryptographic hashes (SHA-512 / SHA-1) of stored blobs against packument integrity.
        #[arg(long, default_value_t = true)]
        check_hashes: bool,
        /// Remove orphaned blobs from storage.
        #[arg(long)]
        prune_orphans: bool,
    },
    /// Export hosted packages and blobs to a directory.
    Export {
        /// Target directory to export packages into.
        #[arg(long)]
        out: PathBuf,
        /// Include auth files (htpasswd, tokens.db) in export.
        #[arg(long, default_value_t = true)]
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
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Validate configuration and report diagnostics.
    Check,
}

pub async fn run_command(config: &Config, command: Command) -> miette::Result<()> {
    match command {
        Command::OciGc { .. } => run_oci_gc_command(config, command).await,
        Command::CacheGc { .. } => run_cache_gc_command(config, command).await,
        Command::ConfigCheck | Command::Config { .. } => run_config_check(),
        Command::Verify { .. } => run_verify_command(config, command).await,
        Command::Export { .. } => run_export_command(config, command).await,
        Command::MigrateStorage { .. } => run_migrate_command(config, command).await,
        Command::ImportHtpasswd { .. } => run_import_command(config, command),
    }
}

async fn run_oci_gc_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::OciGc { registry, dry_run, min_age_secs } = command else {
        return Ok(());
    };
    run_oci_gc(config, registry, dry_run, min_age_secs).await
}

async fn run_cache_gc_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::CacheGc {
        dry_run,
        max_size,
        max_age_secs,
        orphans_only,
    } = command
    else {
        return Ok(());
    };
    run_cache_gc(config, dry_run, max_size, max_age_secs, orphans_only).await
}

async fn run_verify_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::Verify { check_hashes, prune_orphans } = command else {
        return Ok(());
    };
    run_verify(config, check_hashes, prune_orphans).await
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
    let options = pnpr::migrate_storage::MigrateStorageOptions {
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
    let Command::ImportHtpasswd { source, target, dry_run } = command else {
        return Ok(());
    };
    run_import_htpasswd(config, source, target, dry_run)
}

fn run_config_check() -> miette::Result<()> {
    let report = pnpr::config_check::check_config(None)?;
    println!("{}", pnpr::config_check::format_report(&report));
    Ok(())
}

async fn run_migrate_storage(
    config: &Config,
    options: pnpr::migrate_storage::MigrateStorageOptions,
) -> miette::Result<()> {
    let report = pnpr::migrate_storage::migrate_local_to_s3(config, &options).await?;
    print!("{}", pnpr::migrate_storage::format_migrate_report(&report));
    Ok(())
}

async fn run_verify(
    config: &Config,
    check_hashes: bool,
    prune_orphans: bool,
) -> miette::Result<()> {
    let options = pnpr::verify::VerifyOptions { check_hashes, prune_orphans };
    let report = pnpr::verify::verify_storage(config, &options).await?;
    print!("{}", pnpr::verify::format_verify_report(&report));
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
    let report = pnpr::export::export_storage(config, &out, include_auth).await?;
    print!("{}", pnpr::export::format_export_report(&report));
    Ok(())
}

fn run_import_htpasswd(
    config: &Config,
    source: PathBuf,
    target: Option<PathBuf>,
    dry_run: bool,
) -> miette::Result<()> {
    let target = target.or_else(|| config.identity.auth.htpasswd.file.clone());
    let options = pnpr::htpasswd_import::HtpasswdImportOptions { source, target, dry_run };
    let report = pnpr::htpasswd_import::import_htpasswd(&options)?;
    print!("{}", pnpr::htpasswd_import::format_import_report(&report));
    Ok(())
}

async fn run_oci_gc(
    config: &Config,
    registry: String,
    dry_run: bool,
    min_age_secs: u64,
) -> miette::Result<()> {
    pnpr::recover_publish_journal(config).await.map_err(|err| redacted_report(&err))?;
    let (blobs, bytes) = pnpr::oci_maintenance::collect_oci_blobs(
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
    dry_run: bool,
    max_size: Option<String>,
    max_age_secs: Option<u64>,
    orphans_only: bool,
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
    let options = pnpr::cache_maintenance::CacheGcOptions {
        dry_run,
        max_size,
        max_age: max_age_secs.map(Duration::from_secs),
        orphans_only,
    };
    let report = pnpr::cache_maintenance::collect_cache(config, &options).await
        .map_err(|err| redacted_report(&err))?;
    tracing::info!(
        namespaces = report.namespaces_removed,
        files = report.files_evicted,
        bytes = report.bytes_reclaimed,
        dry_run,
        "cache collection completed",
    );
    Ok(())
}

fn redacted_report(err: &RegistryError) -> miette::Report {
    let message = err.log_message();
    miette::miette!("{message}")
}
