use pnpr_config::{
    BackendConfig, CACHE_SECRET_MARKER, Config, HostedStoreConfig, LibsqlSettings, MetadataBackend,
    SqlBackendSettings, cache_secret_fingerprint,
};
use pnpr_error::{RegistryError, Result};
use std::{
    ffi::OsStr,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use url::Url;

const SHARED_BACKEND_SECRET_REQUIRED: &str =
    "`secret` or `secrets.cache` must be configured when using shared storage or database backends";
pub(crate) const PERSISTENT_CACHE_SECRET_REQUIRED: &str = "`secret` or `secrets.cache` must be \
     configured when the cache directory persists across restarts or is shared";
const CACHE_OVERLAPS_HOSTED_STORAGE: &str =
    "cache path must not equal or contain the storage directory";
const CACHE_CONTAINS_METADATA_DB: &str =
    "database.url SQLite path must not live inside the disposable cache directory";
const CACHE_CONTAINS_REPLICA_DB: &str =
    "libSQL replicaPath must not live inside the disposable cache directory";
const CACHE_CONTAINS_AUTH_FILE: &str =
    "auth file path must not live inside the disposable cache directory";
const CACHE_CONTAINS_OSV_DB: &str =
    "osv.path must not live inside or contain the disposable cache directory";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheDirCheck {
    Unmarked,
    ClaimedWithMarker,
}

/// Validate startup settings without opening databases or creating files.
///
/// # Errors
///
/// A server setting is invalid or its database driver is unavailable.
pub(crate) fn validate_startup_config(config: &mut Config) -> Result<()> {
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    config.http.ensure_valid_metrics()?;
    validate_metadata_backend(&config.storage.metadata)?;
    validate_identity_backend(&config.identity.backend)?;
    validate_cache_paths(config)?;
    validate_cache_secret(config)?;
    Ok(())
}

pub(crate) fn validate_cache_paths(config: &Config) -> Result<()> {
    let norm_cache = normalize_config_path(&config.storage.cache_dir)?;
    let norm_hosted = normalize_config_path(&config.storage.hosted_dir)?;
    if norm_hosted.starts_with(&norm_cache) {
        return Err(invalid(CACHE_OVERLAPS_HOSTED_STORAGE));
    }
    validate_metadata_cache_overlap(&config.storage.metadata, &norm_cache)?;
    if let BackendConfig::Libsql(settings) = &config.identity.backend {
        reject_path_inside_cache(
            settings.replica_path.as_deref(),
            &norm_cache,
            CACHE_CONTAINS_REPLICA_DB,
        )?;
    }
    for auth_file in [&config.identity.auth.users.file, &config.identity.auth.tokens.file] {
        reject_path_inside_cache(auth_file.as_deref(), &norm_cache, CACHE_CONTAINS_AUTH_FILE)?;
    }
    validate_osv_cache_overlap(config.osv.path.as_deref(), &norm_cache)?;
    Ok(())
}

fn validate_metadata_cache_overlap(metadata: &MetadataBackend, norm_cache: &Path) -> Result<()> {
    match metadata {
        MetadataBackend::Sqlite(db_path) => reject_path_inside_cache(
            Some(db_path.as_path()),
            norm_cache,
            CACHE_CONTAINS_METADATA_DB,
        ),
        MetadataBackend::Libsql(settings) => reject_path_inside_cache(
            settings.replica_path.as_deref(),
            norm_cache,
            CACHE_CONTAINS_REPLICA_DB,
        ),
        MetadataBackend::Postgres(_) | MetadataBackend::Mysql(_) => Ok(()),
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

fn validate_osv_cache_overlap(osv_path: Option<&Path>, norm_cache: &Path) -> Result<()> {
    if let Some(path) = osv_path
        && !path.as_os_str().is_empty()
    {
        let norm_osv = normalize_config_path(path)?;
        if norm_osv.starts_with(norm_cache) || norm_cache.starts_with(&norm_osv) {
            return Err(invalid(CACHE_CONTAINS_OSV_DB));
        }
    }
    Ok(())
}

fn normalize_config_path(path: &Path) -> Result<PathBuf> {
    Ok(pnpm_fs::resolve_path_with_symlinks(path)?)
}

pub(crate) fn is_ignorable_cache_entry(name: &OsStr, check: CacheDirCheck) -> bool {
    if name == "lost+found" {
        return true;
    }
    match check {
        CacheDirCheck::Unmarked => false,
        CacheDirCheck::ClaimedWithMarker => {
            name == CACHE_SECRET_MARKER || is_cache_secret_temp(name)
        }
    }
}

fn is_cache_secret_temp(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".cache-secret.tmp.")
        && pnpr_storage::is_atomic_write_temp_path(Path::new(name))
}

pub(crate) fn validate_cache_secret(config: &Config) -> Result<()> {
    validate_cache_paths(config)?;
    if config.has_configured_secret {
        return Ok(());
    }
    if uses_shared_backend(config) {
        return Err(invalid(SHARED_BACKEND_SECRET_REQUIRED));
    }
    validate_ephemeral_cache_dir(&config.storage.cache_dir, &config.cache_secret)
}

fn uses_shared_backend(config: &Config) -> bool {
    !matches!(config.storage.hosted_backend, HostedStoreConfig::Fs)
        || !matches!(config.storage.metadata, MetadataBackend::Sqlite(_))
        || !matches!(config.identity.backend, BackendConfig::Local)
}

fn validate_ephemeral_cache_dir(cache_dir: &Path, cache_secret: &[u8]) -> Result<()> {
    let marker = cache_dir.join(CACHE_SECRET_MARKER);
    match fs::read_to_string(&marker) {
        Ok(stored) if stored.trim() == cache_secret_fingerprint(cache_secret) => return Ok(()),
        Ok(_) => return Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED)),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !is_ignorable_cache_entry(&entry.file_name(), CacheDirCheck::Unmarked) {
            return Err(invalid(PERSISTENT_CACHE_SECRET_REQUIRED));
        }
    }
    Ok(())
}

fn validate_metadata_backend(metadata: &MetadataBackend) -> Result<()> {
    match metadata {
        MetadataBackend::Sqlite(path) if path.as_os_str().is_empty() => {
            Err(invalid("database.url `sqlite:` needs a path after the colon"))
        }
        MetadataBackend::Sqlite(_) => Ok(()),
        MetadataBackend::Postgres(settings) => validate_postgres(settings),
        MetadataBackend::Mysql(settings) => validate_mysql(settings),
        MetadataBackend::Libsql(settings) => validate_libsql(settings),
    }
}

fn validate_identity_backend(backend: &BackendConfig) -> Result<()> {
    match backend {
        BackendConfig::Local => Ok(()),
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
        return Err(invalid("database.maxConnections must be greater than 0"));
    }
    if settings.replica_path.is_some() && settings.sync_interval_secs == Some(0) {
        return Err(invalid(
            "database.syncInterval must be at least 1s with a replicaPath: it bounds how stale a \
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
    pnpr_libsql::validate_settings(settings, "database")?;
    Ok(())
}

fn validate_sql_pool(settings: &SqlBackendSettings) -> Result<()> {
    if settings.max_connections == Some(0) {
        return Err(invalid("database.maxConnections must be greater than 0"));
    }
    if settings.timeout.is_zero() {
        return Err(invalid("database.timeout must be greater than 0"));
    }
    if settings.startup_timeout.is_zero() {
        return Err(invalid("database.startupTimeout must be greater than 0"));
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
    invalid("database.url is not a valid URL")
}

fn missing_driver() -> RegistryError {
    invalid("this build does not include the configured database driver")
}

fn invalid(reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: reason.to_string() }
}

#[cfg(test)]
mod tests;
