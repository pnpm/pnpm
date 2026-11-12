use super::{
    ByteSize, Config, ConfigFile, HttpConfig, Interval, MetricsConfig, Path, RegistryError,
    SocketAddr, build_cors_config, build_oci_config, build_package_documents_config,
    build_ui_config, cdn, parse_interval, parse_trusted_proxies,
};
use crate::byte_size::parse_positive_byte_size;

pub(super) fn build_http_config(
    file: &mut ConfigFile,
    base_dir: &Path,
    listen: Option<SocketAddr>,
    public_url: Option<String>,
) -> Result<HttpConfig, RegistryError> {
    let listen = match listen.or(file.listen) {
        Some(listen) => listen,
        None => Config::DEFAULT_LISTEN.parse().expect("DEFAULT_LISTEN is a socket address"),
    };
    let public_url = public_url.or_else(|| file.public_url.take());
    let cors = build_cors_config(std::mem::take(&mut file.cors))?;
    let ui = build_ui_config(std::mem::take(&mut file.ui), base_dir);
    let trusted_proxies = parse_trusted_proxies(&file.trusted_proxies.networks)?;
    let trusted_proxy_header = file.trusted_proxies.header.unwrap_or_default();
    let request_timeout = build_request_timeout(file.request_timeout.as_ref())?;
    let shutdown_timeout = build_shutdown_timeout(file.shutdown_timeout.take())?;
    let packument_ttl = build_packument_ttl(file.packument_ttl.take())?;
    let max_tarball_bytes = build_max_tarball_bytes(file.max_tarball_size.take())?;
    Ok(HttpConfig {
        listen,
        public_url: public_url.unwrap_or_else(|| format!("http://{listen}")),
        cors,
        ui,
        oci: build_oci_config(std::mem::take(&mut file.oci))?,
        packument_ttl,
        max_tarball_bytes,
        trusted_proxies,
        trusted_proxy_header,
        request_timeout,
        shutdown_timeout,
        metrics: build_metrics_config(file, listen)?,
        package_documents: build_package_documents_config(file.package_documents.take())?,
        cloudflare_cdn: cdn::build(file.cloudflare_cdn.take())?,
    })
}

fn build_request_timeout(
    raw: Option<&Interval>,
) -> Result<Option<std::time::Duration>, RegistryError> {
    let Some(interval) = raw else {
        return Ok(Some(HttpConfig::DEFAULT_REQUEST_TIMEOUT));
    };
    let duration = parse_interval(&interval.0)
        .ok_or_else(|| RegistryError::InvalidConfig {
            reason: format!("http.requestTimeout {:?} is not a valid interval", interval.0),
        })?;
    Ok((!duration.is_zero()).then_some(duration))
}

/// The `shutdownTimeout:` key as a duration, defaulting to
/// [`HttpConfig::DEFAULT_SHUTDOWN_TIMEOUT`]. Zero is refused: a
/// zero-length drain would abort in-flight requests as soon as shutdown starts.
fn build_shutdown_timeout(raw: Option<Interval>) -> Result<std::time::Duration, RegistryError> {
    let Some(Interval(raw)) = raw else {
        return Ok(HttpConfig::DEFAULT_SHUTDOWN_TIMEOUT);
    };
    let duration = parse_interval(&raw)
        .ok_or_else(|| RegistryError::InvalidConfig {
            reason: format!("shutdownTimeout {raw:?} is not a valid interval"),
        })?;
    if duration.is_zero() {
        return Err(RegistryError::InvalidConfig {
            reason: "shutdownTimeout must be greater than zero".to_string(),
        });
    }
    Ok(duration)
}

/// The `packumentTtl:` key as a duration, defaulting to
/// [`Config::DEFAULT_PACKUMENT_TTL`].
fn build_packument_ttl(raw: Option<Interval>) -> Result<std::time::Duration, RegistryError> {
    let Some(Interval(raw)) = raw else {
        return Ok(Config::DEFAULT_PACKUMENT_TTL);
    };
    parse_interval(&raw)
        .ok_or_else(|| RegistryError::InvalidConfig {
            reason: format!("packumentTtl {raw:?} is not a valid interval"),
        })
}

/// The `maxTarballSize:` key as bytes, defaulting to
/// [`HttpConfig::DEFAULT_MAX_TARBALL_BYTES`].
fn build_max_tarball_bytes(raw: Option<ByteSize>) -> Result<u64, RegistryError> {
    raw.map_or(Ok(HttpConfig::DEFAULT_MAX_TARBALL_BYTES), |raw| {
        parse_positive_byte_size("maxTarballSize", &raw)
    })
}

fn build_metrics_config(
    file: &mut ConfigFile,
    listen: SocketAddr,
) -> Result<Option<MetricsConfig>, RegistryError> {
    let metrics = file.metrics.take();
    if let Some(metrics) = &metrics {
        metrics.validate(listen)?;
    }
    Ok(metrics)
}
