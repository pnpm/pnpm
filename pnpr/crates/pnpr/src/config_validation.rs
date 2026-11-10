use pnpr_config::{Config, MetadataBackend};

/// Validate startup settings without opening databases or creating files.
///
/// # Errors
///
/// A server setting is invalid or its database driver is unavailable.
pub(crate) fn validate_startup_config(config: &mut Config) -> pnpr_error::Result<()> {
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    config.http.ensure_valid_metrics()?;
    validate_database_url(&config.storage.metadata)?;
    let available = match &config.storage.metadata {
        MetadataBackend::Sqlite(_) => true,
        MetadataBackend::Postgres(_) => cfg!(feature = "backend-postgres"),
        MetadataBackend::Mysql(_) => cfg!(feature = "backend-mysql"),
        MetadataBackend::Libsql(settings) => {
            #[cfg(feature = "backend-libsql")]
            pnpr_libsql::validate_settings(settings, "database")?;
            #[cfg(not(feature = "backend-libsql"))]
            let _ = settings;
            cfg!(feature = "backend-libsql")
        }
    };
    if !available {
        return Err(pnpr_error::RegistryError::InvalidConfig {
            reason: "this build does not include the configured database driver".to_string(),
        });
    }
    Ok(())
}

fn validate_database_url(metadata: &MetadataBackend) -> pnpr_error::Result<()> {
    let target = match metadata {
        MetadataBackend::Sqlite(_) => None,
        MetadataBackend::Postgres(settings) | MetadataBackend::Mysql(settings) => {
            if settings.max_connections == Some(0) {
                return Err(pnpr_error::RegistryError::InvalidConfig {
                    reason: "database.maxConnections must be greater than 0".to_string(),
                });
            }
            Some(&settings.url)
        }
        MetadataBackend::Libsql(settings) => Some(&settings.url),
    };
    if target.is_some_and(|target| url::Url::parse(target).is_err()) {
        return Err(pnpr_error::RegistryError::InvalidConfig {
            reason: "database.url is not a valid URL".to_string(),
        });
    }
    Ok(())
}
