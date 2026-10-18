use crate::{Config, RegistryError};
use clap::{Args, Subcommand, builder::BoolishValueParser};
use pnpm_diagnostics::{print, println};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

pub fn parse_cli_interval(raw: &str) -> std::result::Result<Duration, String> {
    pnpr_config::parse_interval(raw)
        .ok_or_else(|| format!("invalid interval {raw:?}; expected e.g. 30s, 5m, 1h, 1d"))
}

#[derive(Debug, Args, Clone)]
pub struct ServerArgs {
    /// Address to bind to.
    #[arg(long, default_value = Config::DEFAULT_LISTEN, env = "PNPR_LISTEN")]
    pub listen: SocketAddr,

    /// URL clients should use to reach this server. Used when
    /// rewriting `dist.tarball` URLs in served packuments. Defaults
    /// to `http://<listen>`.
    #[arg(long, env = "PNPR_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Duration before a cached packument is considered stale and
    /// refetched (e.g. "5m", "300s"). A registry's `maxage` overrides it.
    #[arg(long, env = "PNPR_PACKUMENT_TTL", alias = "packument-ttl-secs", value_parser = parse_cli_interval)]
    pub packument_ttl: Option<Duration>,

    #[command(flatten)]
    pub osv_options: OsvArgs,

    #[command(flatten)]
    pub features: FeatureArgs,
}

impl Default for ServerArgs {
    fn default() -> Self {
        Self {
            listen: Config::DEFAULT_LISTEN.parse().unwrap(),
            public_url: None,
            packument_ttl: None,
            osv_options: OsvArgs::default(),
            features: FeatureArgs::default(),
        }
    }
}

impl ServerArgs {
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        if other.listen.to_string() != Config::DEFAULT_LISTEN {
            self.listen = other.listen;
        }
        if other.public_url.is_some() {
            self.public_url = other.public_url;
        }
        if other.packument_ttl.is_some() {
            self.packument_ttl = other.packument_ttl;
        }
        if other.osv_options.osv.is_some() {
            self.osv_options.osv = other.osv_options.osv;
        }
        if other.osv_options.osv_db.is_some() {
            self.osv_options.osv_db = other.osv_options.osv_db;
        }
        if other.features.disable_registry {
            self.features.disable_registry = true;
        }
        if other.features.disable_resolver {
            self.features.disable_resolver = true;
        }
        if other.features.disable_artifacts {
            self.features.disable_artifacts = true;
        }
        if other.features.disable_pipeline {
            self.features.disable_pipeline = true;
        }
        self
    }
}

#[derive(Debug, Args, Clone, Default)]
pub struct OsvArgs {
    /// Enable local OSV npm vulnerability checks. Requires a local OSV
    /// npm database zip at `--osv-db` or `<cache>/osv/npm/all.zip`.
    #[arg(long, env = "PNPR_OSV", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub osv: Option<bool>,

    /// Path to the local OSV npm database zip or extracted JSON directory.
    #[arg(long, env = "PNPR_OSV_DB")]
    pub osv_db: Option<PathBuf>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct FeatureArgs {
    /// Disable the npm-registry surface (packument/tarball reads, publish,
    /// unpublish, dist-tag, search) on this tier. Without the flag the
    /// surface is served whenever the loaded config declares at least one
    /// registry under `registries:`.
    #[arg(long, env = "PNPR_DISABLE_REGISTRY", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_registry: bool,

    /// Disable the install-accelerator surface (`/-/pnpr`, `/-/pnpr/v0/resolve`,
    /// `/-/pnpr/v0/verify-lockfile`). Overrides `resolver.enabled` from the
    /// loaded config.
    #[arg(long, env = "PNPR_DISABLE_RESOLVER", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_resolver: bool,

    /// Disable the signed shared-artifact surface. Overrides
    /// `artifacts.enabled` from the loaded config.
    #[arg(long, env = "PNPR_DISABLE_ARTIFACTS", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_artifacts: bool,

    /// Disable the pipeline run-record surface (`/-/pnpr/v0/pipeline`). Overrides
    /// `pipeline.enabled` from the loaded config.
    #[arg(long, env = "PNPR_DISABLE_PIPELINE", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_pipeline: bool,
}

impl FeatureArgs {
    #[must_use]
    pub fn feature_overrides(&self) -> pnpr_config::FeatureOverrides {
        pnpr_config::FeatureOverrides {
            disable_registry: self.disable_registry,
            disable_resolver: self.disable_resolver,
            disable_artifacts: self.disable_artifacts,
            disable_pipeline: self.disable_pipeline,
        }
    }
}

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
        Command::Serve(..) => {
            unreachable!("Serve is handled directly by main()")
        }
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
    run_cache_gc(config, dry_run, max_size, max_age.map(|d| d.as_secs()), orphans_only).await
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
    let options = crate::cache_maintenance::CacheGcOptions {
        dry_run,
        max_size,
        max_age: max_age_secs.map(Duration::from_secs),
        orphans_only,
    };
    let report = crate::cache_maintenance::collect_cache(config, &options).await
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
