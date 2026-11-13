pub mod args;
pub use args::{FeatureArgs, OsvArgs, ServerArgs, parse_cli_interval};

use crate::{
    Config, RegistryError,
    cache_maintenance::{CacheGcEffect, CacheGcScope},
    server::{Credentials, FileReferences, with_stored_routing},
};
use clap::Subcommand;
use pnpm_diagnostics::{print, println};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the registry server (default when no subcommand is specified).
    Serve(ServerArgs),
    /// Remove archives no hosted package references. Safe while the
    /// registry serves and other replicas publish.
    Gc {
        /// Hosted registry from the config to collect in; repeat for more.
        /// Every hosted registry when omitted.
        #[arg(long)]
        registry: Vec<String>,
        /// Report reclaimable archives without removing them.
        #[arg(long)]
        dry_run: bool,
        /// How long ago an archive must have been stored, or an image blob
        /// uploaded, before an unfinished publish or push of it is fenced off
        /// and it is reclaimed (e.g. "1d", "86400s", or bare seconds).
        #[arg(long, default_value = "1d", value_parser = parse_cli_interval)]
        min_age: Duration,
    },
    /// Reclaim cached upstream and resolver files, and prune orphaned namespaces.
    CacheGc {
        /// Report reclaimable entries without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Maximum total cache size to retain (e.g. "10GB", "500MB", or raw bytes).
        #[arg(long)]
        max_size: Option<String>,
        /// Maximum file age (e.g. "30d", "1h", or bare seconds). Entries older than this are deleted.
        #[arg(long, value_parser = parse_cli_interval)]
        max_age: Option<Duration>,
        /// Only prune orphaned upstream namespaces without evicting active cache files.
        #[arg(long)]
        orphans_only: bool,
    },
    /// Configuration management commands.
    Config {
        #[command(subcommand)]
        action: ConfigSubcommand,
    },
    /// Verify every document against its blobs (tarballs, attestations), and
    /// report orphaned blobs. `pnpr gc` removes them.
    Verify {
        /// Skip cryptographic hash verification (SHA-512 / SHA-1) of stored blobs against packument integrity.
        #[arg(long)]
        skip_hashes: bool,
    },
    /// Export hosted packages and blobs to a directory.
    Export {
        /// Target directory to export packages into.
        #[arg(long)]
        out: PathBuf,
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
    /// Import users from an existing Verdaccio or Apache htpasswd file into
    /// the SQLite database.
    ImportHtpasswd {
        /// Path to source htpasswd file.
        #[arg(long)]
        source: PathBuf,
        /// Report what would be imported without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Overwrite the credentials of existing users whose credentials differ.
        #[arg(long)]
        overwrite: bool,
    },
    /// Add or update an account and print its immutable ID.
    AddUser {
        /// Username of the account to create or update.
        username: String,
        #[arg(skip)]
        password: Option<String>,
    },
    /// Read or replace the registries the database stores.
    Registries {
        #[command(subcommand)]
        action: RegistriesSubcommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum RegistriesSubcommand {
    /// Print the stored registries as YAML: the `registries`,
    /// `defaultRegistry`, and `routes.public` keys.
    Export {
        /// Write the YAML to this file instead of standard output.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Replace the stored registries with the ones a YAML file declares, as one
    /// new revision.
    Import {
        /// YAML file with the `registries`, `defaultRegistry`, and
        /// `routes.public` keys.
        file: PathBuf,
        /// Check the file and report what would change without storing it.
        #[arg(long, conflicts_with = "check")]
        dry_run: bool,
        /// Compare the file with the stored registries without storing it,
        /// and exit with code 1 when they differ.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Validate configuration and report diagnostics.
    Check,
}

pub async fn run_command(config: &Config, command: Command) -> miette::Result<()> {
    let config = &with_registries(config, &command).await?;
    match command {
        Command::Serve(..) => Err(miette::miette!(
            "`serve` is not a maintenance command; start the server with `pnpr::serve`"
        )),
        Command::Gc { .. } => run_gc_command(config, command).await,
        Command::CacheGc { .. } => run_cache_gc_command(config, command).await,
        Command::Config { .. } => run_config_check(),
        Command::Verify { .. } => run_verify_command(config, command).await,
        Command::Export { .. } => run_export_command(config, command).await,
        Command::MigrateStorage { .. } => run_migrate_command(config, command).await,
        Command::ImportHtpasswd { .. } => run_import_command(config, command).await,
        Command::AddUser { username, password } => {
            run_add_user_command(config, username, password).await
        }
        Command::Registries { action } => crate::registries_command::run(config, action).await,
    }
}

/// `config` with the stored registries for the commands that walk them, when
/// the database stores them.
async fn with_registries(config: &Config, command: &Command) -> miette::Result<Config> {
    let credentials = match command {
        Command::CacheGc { .. } => Credentials::Resolve,
        Command::Gc { .. }
        | Command::Verify { .. }
        | Command::Export { .. }
        | Command::MigrateStorage { .. } => Credentials::Skip,
        _ => return Ok(config.clone()),
    };
    with_stored_routing(config, credentials, FileReferences::Warn).await
        .map_err(|err| redacted_report(&err))
}

async fn run_gc_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::Gc { registry, dry_run, min_age } = command else {
        return Ok(());
    };
    let options = crate::archive_gc::GcOptions { registries: registry, min_age, dry_run };
    run_gc(config, &options).await
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
    let Command::Verify { skip_hashes } = command else {
        return Ok(());
    };
    run_verify(config, !skip_hashes).await
}

async fn run_export_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::Export { out } = command else {
        return Ok(());
    };
    run_export(config, out).await
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

async fn run_import_command(config: &Config, command: Command) -> miette::Result<()> {
    let Command::ImportHtpasswd { source, dry_run, overwrite } = command else {
        return Ok(());
    };
    let options = crate::htpasswd_import::HtpasswdImportOptions { source, dry_run, overwrite };
    let report = crate::htpasswd_import::import_htpasswd(config, &options).await?;
    print!("{}", crate::htpasswd_import::format_import_report(&report));
    Ok(())
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

async fn run_verify(config: &Config, check_hashes: bool) -> miette::Result<()> {
    let options = crate::verify::VerifyOptions { check_hashes };
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

async fn run_export(config: &Config, out: PathBuf) -> miette::Result<()> {
    let report = crate::export::export_storage(config, &out).await?;
    print!("{}", crate::export::format_export_report(&report));
    Ok(())
}

async fn run_gc(config: &Config, options: &crate::archive_gc::GcOptions) -> miette::Result<()> {
    let report = crate::archive_gc::collect_archives(config, options).await
        .map_err(|err| redacted_report(&err))?;
    let verb = if options.dry_run { "would remove" } else { "removed" };
    println!("Archive collection {verb} {} archives ({} bytes).", report.archives, report.bytes);
    Ok(())
}

async fn run_cache_gc(
    config: &Config,
    effect: CacheGcEffect,
    max_size: Option<String>,
    max_age_secs: Option<u64>,
    scope: CacheGcScope,
) -> miette::Result<()> {
    crate::config_validation::validate_cache_secret(config).map_err(|err| redacted_report(&err))?;
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
    let password = match password {
        Some(password) => password,
        None => read_password()?,
    };
    validate_password(&password)?;
    let auth = pnpr_auth::AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .map_err(|err| miette::miette!("could not connect to auth backend: {err}"))?;
    let id = match auth
        .put_user(&username, &password)
        .await
        .map_err(|err| miette::miette!("{err}"))?
    {
        Some(pnpr_auth::UserPut::Created { user_id }) => {
            println!("User '{username}' created successfully.");
            user_id
        }
        Some(pnpr_auth::UserPut::PasswordChanged { user_id }) => {
            println!("Password for user '{username}' updated successfully.");
            user_id
        }
        None => return Err(miette::miette!("failed to create or update user '{username}'")),
    };
    println!("Account ID: {id}");
    Ok(())
}

fn validate_password(password: &str) -> miette::Result<()> {
    if password.is_empty() {
        return Err(miette::miette!("password cannot be empty"));
    }
    if password.len() > pnpr_auth::MAX_PASSWORD_BYTES {
        return Err(miette::miette!(
            "password cannot exceed {} bytes",
            pnpr_auth::MAX_PASSWORD_BYTES,
        ));
    }
    Ok(())
}
