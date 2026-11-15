use pnpr_config::{
    BackendConfig, CACHE_SECRET_MARKER, Config, DatabaseBackend, HostedStoreConfig, LibsqlSettings,
    SqlBackendSettings, cache_secret_fingerprint, default_osv_path,
};
use pnpr_error::{RegistryError, Result};
use std::{
    ffi::OsStr,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use url::Url;

const SHARED_BACKEND_SECRET_REQUIRED: &str =
    "`secret` or `secret.cache` must be configured when using shared storage or database backends";
pub(crate) const PERSISTENT_CACHE_SECRET_REQUIRED: &str = "`secret` or `secret.cache` must be \
     configured when the cache directory persists across restarts or is shared";
pub(crate) const MAX_CACHE_SECRET_MARKER_BYTES: usize = 256;
pub(crate) const PENDING_CACHE_CLAIM: &str = "pending";
const CACHE_OVERLAPS_HOSTED_STORAGE: &str =
    "cache path must not equal or contain the storage directory";
const CACHE_CONTAINS_METADATA_DB: &str =
    "storage.database.url SQLite path must not live inside the disposable cache directory";
const CACHE_CONTAINS_REPLICA_DB: &str =
    "libSQL replicaPath must not live inside the disposable cache directory";
const CACHE_CONTAINS_OSV_DB: &str =
    "osv.path must not live inside or contain the disposable cache directory";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheDirCheck {
    Unmarked,
    ClaimedWithMarker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingClaimPolicy {
    Reject,
    DeferToClaim,
}

/// Validate startup settings without opening databases or creating files.
///
/// # Errors
///
/// A server setting is invalid or its database driver is unavailable.
pub(crate) fn validate_startup_config(config: &mut Config) -> Result<()> {
    validate_startup_config_inner(config, PendingClaimPolicy::Reject)
}

pub(crate) fn validate_serving_startup_config(config: &mut Config) -> Result<()> {
    validate_startup_config_inner(config, PendingClaimPolicy::DeferToClaim)
}

fn validate_startup_config_inner(
    config: &mut Config,
    pending_policy: PendingClaimPolicy,
) -> Result<()> {
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    config.http.ensure_valid_metrics()?;
    pnpr_route::validate_public_routes(&config.routing.route_policy)
        .map_err(|reason| RegistryError::InvalidConfig { reason })?;
    if config.http.shutdown_timeout.is_zero() {
        return Err(invalid("shutdownTimeout must be greater than zero"));
    }
    validate_database_backend(&config.storage.database)?;
    validate_identity_backend(&config.identity.backend)?;
    validate_cache_paths(config)?;
    validate_cache_secret_inner(config, pending_policy)?;
    Ok(())
}

pub(crate) fn validate_cache_paths(config: &Config) -> Result<()> {
    let norm_cache = normalize_config_path(&config.storage.cache_dir)?;
    let norm_hosted = normalize_config_path(&config.storage.hosted_dir)?;
    if norm_hosted.starts_with(&norm_cache) {
        return Err(invalid(CACHE_OVERLAPS_HOSTED_STORAGE));
    }
    validate_database_cache_overlap(&config.storage.database, &norm_cache)?;
    if let BackendConfig::Libsql(settings) = &config.identity.backend {
        reject_path_inside_cache(
            settings.replica_path.as_deref(),
            &norm_cache,
            CACHE_CONTAINS_REPLICA_DB,
        )?;
    }
    let default_osv;
    let effective_osv = match config.osv.path.as_deref() {
        Some(path) if !path.as_os_str().is_empty() => path,
        _ => {
            default_osv = default_osv_path(&config.storage.hosted_dir);
            &default_osv
        }
    };
    validate_osv_cache_overlap(effective_osv, &norm_cache)?;
    Ok(())
}

fn validate_database_cache_overlap(metadata: &DatabaseBackend, norm_cache: &Path) -> Result<()> {
    match metadata {
        DatabaseBackend::Sqlite(db_path) => reject_path_inside_cache(
            Some(db_path.as_path()),
            norm_cache,
            CACHE_CONTAINS_METADATA_DB,
        ),
        DatabaseBackend::Libsql(settings) => reject_path_inside_cache(
            settings.replica_path.as_deref(),
            norm_cache,
            CACHE_CONTAINS_REPLICA_DB,
        ),
        DatabaseBackend::Postgres(_) | DatabaseBackend::Mysql(_) => Ok(()),
    }
}

fn reject_path_inside_cache(
    candidate: Option<&Path>,
    norm_cache: &Path,
    reason: &str,
) -> Result<()> {
    if let Some(path) = candidate
        && !path.as_os_str().is_empty()
        && normalize_config_path(path)?.starts_with(norm_cache)
    {
        return Err(invalid(reason));
    }
    Ok(())
}

fn validate_osv_cache_overlap(osv_path: &Path, norm_cache: &Path) -> Result<()> {
    let norm_osv = normalize_config_path(osv_path)?;
    if norm_osv.starts_with(norm_cache) || norm_cache.starts_with(&norm_osv) {
        return Err(invalid(CACHE_CONTAINS_OSV_DB));
    }
    Ok(())
}

fn normalize_config_path(path: &Path) -> Result<PathBuf> {
    Ok(pnpm_fs::resolve_path_with_symlinks(path)?)
}

pub(crate) fn is_ignorable_cache_entry(name: &OsStr, check: CacheDirCheck) -> bool {
    if name == "lost+found" || is_cache_secret_temp(name) {
        return true;
    }
    match check {
        CacheDirCheck::Unmarked => false,
        CacheDirCheck::ClaimedWithMarker => name == CACHE_SECRET_MARKER,
    }
}

fn is_cache_secret_temp(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".cache-secret.tmp.")
        && pnpr_storage::is_atomic_write_temp_path(Path::new(name))
}

pub(crate) fn validate_cache_paths_and_backend(config: &Config) -> Result<()> {
    validate_cache_paths(config)?;
    if !config.has_configured_secret && uses_shared_backend(config) {
        return Err(invalid(SHARED_BACKEND_SECRET_REQUIRED));
    }
    Ok(())
}

pub(crate) fn validate_cache_secret(config: &Config) -> Result<()> {
    validate_cache_secret_inner(config, PendingClaimPolicy::Reject)
}

pub(crate) fn validate_cache_secret_for_claim(config: &Config) -> Result<()> {
    validate_cache_secret_inner(config, PendingClaimPolicy::DeferToClaim)
}

fn validate_cache_secret_inner(config: &Config, pending_policy: PendingClaimPolicy) -> Result<()> {
    validate_cache_paths_and_backend(config)?;
    if config.has_configured_secret {
        return Ok(());
    }
    validate_ephemeral_cache_dir(&config.storage.cache_dir, &config.cache_secret, pending_policy)
}

fn uses_shared_backend(config: &Config) -> bool {
    let shared_hosted = match &config.storage.hosted_backend {
        HostedStoreConfig::Fs => false,
        HostedStoreConfig::S3(_) | HostedStoreConfig::ObjectStore { .. } => true,
    };
    let shared_metadata = match &config.storage.database {
        DatabaseBackend::Sqlite(_) => false,
        DatabaseBackend::Libsql(_) | DatabaseBackend::Postgres(_) | DatabaseBackend::Mysql(_) => {
            true
        }
    };
    let shared_identity = match &config.identity.backend {
        BackendConfig::Sqlite(_) => false,
        BackendConfig::Libsql(_) | BackendConfig::Postgres(_) | BackendConfig::Mysql(_) => true,
    };
    shared_hosted || shared_metadata || shared_identity
}

fn validate_ephemeral_cache_dir(
    cache_dir: &Path,
    cache_secret: &[u8],
    pending_policy: PendingClaimPolicy,
) -> Result<()> {
    let dir = match pnpm_fs::OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let marker = OsStr::new(CACHE_SECRET_MARKER);
    if dir.has_symlink_child(marker)? {
        return Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED));
    }
    match read_ephemeral_marker(&dir, marker)? {
        Some(bytes) => validate_ephemeral_marker(&dir, &bytes, cache_secret, pending_policy),
        None => ensure_only_ignorable_entries(&dir, CacheDirCheck::Unmarked),
    }
}

fn read_ephemeral_marker(dir: &pnpm_fs::OpenDir, marker: &OsStr) -> Result<Option<Vec<u8>>> {
    match dir.read_child_file_bounded(marker, MAX_CACHE_SECRET_MARKER_BYTES) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == ErrorKind::InvalidData => {
            Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED))
        }
        Err(err) => Err(err.into()),
    }
}

fn validate_ephemeral_marker(
    dir: &pnpm_fs::OpenDir,
    bytes: &[u8],
    cache_secret: &[u8],
    pending_policy: PendingClaimPolicy,
) -> Result<()> {
    let Ok(raw) = std::str::from_utf8(bytes) else {
        return Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED));
    };
    let trimmed = raw.trim();
    if trimmed == cache_secret_fingerprint(cache_secret) {
        return Ok(());
    }
    match pending_policy {
        PendingClaimPolicy::Reject => Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED)),
        PendingClaimPolicy::DeferToClaim if trimmed == PENDING_CACHE_CLAIM => {
            ensure_only_ignorable_entries(dir, CacheDirCheck::ClaimedWithMarker)
        }
        PendingClaimPolicy::DeferToClaim => Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED)),
    }
}

pub(crate) fn ensure_only_ignorable_entries(
    dir: &pnpm_fs::OpenDir,
    check: CacheDirCheck,
) -> Result<()> {
    for name in dir.entry_names()? {
        if !is_ignorable_cache_entry(&name, check) {
            return Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED));
        }
    }
    Ok(())
}

fn validate_database_backend(metadata: &DatabaseBackend) -> Result<()> {
    match metadata {
        DatabaseBackend::Sqlite(path) if path.as_os_str().is_empty() => {
            Err(invalid("storage.database.url `sqlite:` needs a path after the colon"))
        }
        DatabaseBackend::Sqlite(_) => Ok(()),
        DatabaseBackend::Postgres(settings) => validate_postgres(settings),
        DatabaseBackend::Mysql(settings) => validate_mysql(settings),
        DatabaseBackend::Libsql(settings) => validate_libsql(settings),
    }
}

fn validate_identity_backend(backend: &BackendConfig) -> Result<()> {
    match backend {
        BackendConfig::Sqlite(_) => Ok(()),
        BackendConfig::Postgres(settings) => validate_postgres(settings),
        BackendConfig::Mysql(settings) => validate_mysql(settings),
        BackendConfig::Libsql(settings) => validate_libsql(settings),
    }
}

fn validate_postgres(settings: &SqlBackendSettings) -> Result<()> {
    validate_sql_pool(settings)?;
    let parsed = parse_hierarchical_url(&settings.url)?;
    if !matches!(parsed.scheme(), "postgres" | "postgresql") {
        return Err(invalid_url());
    }
    if !cfg!(feature = "backend-postgres") {
        return Err(missing_driver());
    }
    #[cfg(feature = "backend-postgres")]
    {
        use std::str::FromStr as _;
        sqlx::postgres::PgConnectOptions::from_str(&settings.url).map_err(|_| invalid_url())?;
    }
    Ok(())
}

fn validate_mysql(settings: &SqlBackendSettings) -> Result<()> {
    validate_sql_pool(settings)?;
    let parsed = parse_hierarchical_url(&settings.url)?;
    if parsed.scheme() != "mysql" {
        return Err(invalid_url());
    }
    if !cfg!(feature = "backend-mysql") {
        return Err(missing_driver());
    }
    #[cfg(feature = "backend-mysql")]
    {
        use std::str::FromStr as _;
        sqlx::mysql::MySqlConnectOptions::from_str(&settings.url).map_err(|_| invalid_url())?;
    }
    Ok(())
}

fn validate_libsql(settings: &LibsqlSettings) -> Result<()> {
    if settings.max_connections == Some(0) {
        return Err(invalid("storage.database.maxConnections must be greater than 0"));
    }
    if settings.replica_path.is_some() && settings.sync_interval_secs == Some(0) {
        return Err(invalid(
            "storage.database.syncInterval must be at least 1s with a replicaPath: it bounds how stale a \
             replica read may be",
        ));
    }
    let parsed = parse_hierarchical_url(&settings.url)?;
    let has_host = parsed
        .host_str()
        .is_some_and(|host| !host.is_empty());
    if !matches!(parsed.scheme(), "libsql" | "http" | "https") || !has_host {
        return Err(invalid_url());
    }
    if !cfg!(feature = "backend-libsql") {
        return Err(missing_driver());
    }
    #[cfg(feature = "backend-libsql")]
    pnpr_libsql::validate_settings(settings, "storage.database")?;
    Ok(())
}

fn validate_sql_pool(settings: &SqlBackendSettings) -> Result<()> {
    if settings.max_connections == Some(0) {
        return Err(invalid("storage.database.maxConnections must be greater than 0"));
    }
    if settings.timeout.is_zero() {
        return Err(invalid("storage.database.timeout must be greater than 0"));
    }
    if settings.startup_timeout.is_zero() {
        return Err(invalid("storage.database.startupTimeout must be greater than 0"));
    }
    Ok(())
}

fn parse_hierarchical_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw).map_err(|_| invalid_url())?;
    let has_authority_prefix = raw
        .get(parsed.scheme().len()..)
        .is_some_and(|rest| rest.starts_with("://"));
    if parsed.cannot_be_a_base() || !has_authority_prefix {
        return Err(invalid_url());
    }
    Ok(parsed)
}

fn invalid_url() -> RegistryError {
    invalid("storage.database.url is not a valid URL")
}

fn missing_driver() -> RegistryError {
    invalid("this build does not include the configured database driver")
}

fn invalid(reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: reason.to_string() }
}

#[cfg(test)]
mod tests;
