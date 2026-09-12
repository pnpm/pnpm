use super::{
    Arc, AuthHeaders, BTreeMap, Config, IntoDiagnostic, Path, PathBuf, RegistryConfig, Result,
    RetryOpts, StreamExt, ThrottledClient, TryStreamExt, fs, is_crates_io, registry_auth, stream,
    validate_package_field,
};
use miette::WrapErr;

const CRATES_IO_DOWNLOAD_BASE: &str = "https://static.crates.io/crates";

/// A registry's `config.json` holds a download template and two URLs. The
/// cap keeps a registry the workspace names from spending the client's
/// memory and cache before the document is even parsed.
const MAX_REGISTRY_CONFIG_BYTES: usize = 64 * 1024;

pub(crate) async fn latest_version(
    config: &Config,
    auth_headers: &AuthHeaders,
    name: &str,
    http_client: &ThrottledClient,
) -> Result<String> {
    let cache_dir = cargo_index_cache_dir(config);
    let index_file = fetch_sparse_index_file(
        name,
        &config.cargo.index_url,
        &cache_dir,
        http_client,
        auth_headers,
        config.offline,
        config.retry_opts(),
    )
    .await?;
    pnpm_cargo_resolver::latest_version(name, &index_file)
        .wrap_err_with(|| format!("select the latest version of crate {name}"))
}

/// The credentials a crate archive download may carry.
///
/// `cargo.indexUrl` is repository-selected and its `config.json` names the
/// download host, so the only credential that may travel is the one
/// configured for the registry itself, never one looked up by the host the
/// registry names. Off the registry's own origin it travels only when the
/// registry sets `auth-required`, which is the same condition under which
/// `cargo` sends its own token. Either way it travels only over TLS or
/// loopback.
///
/// A Cargo install runs from the CLI, which installs no route hook, so the
/// anonymous headers deny nothing a hook would have allowed.
pub(super) fn download_auth_headers(
    config: &Config,
    registry_config: &RegistryConfig,
) -> Arc<AuthHeaders> {
    // The registry serves its own downloads: every credential configured
    // for it applies to them as it does to its index.
    if same_origin(&registry_config.dl, &config.cargo.index_url) {
        return Arc::new((*config.auth_headers).clone().with_secure_transport());
    }
    let credential = registry_config
        .auth_required
        .then(|| config.auth_headers.for_secure_url(&config.cargo.index_url))
        .flatten();
    let Some((credential, origin)) = credential.zip(origin_of(&registry_config.dl)) else {
        return Arc::new(AuthHeaders::default());
    };
    let mut auth_headers = AuthHeaders::default().with_secure_transport();
    auth_headers.insert_url_header(&origin, credential);
    Arc::new(auth_headers)
}

/// Whether both URLs are absolute and share a scheme, host, and port. A URL
/// that does not parse, or whose scheme has no host, matches nothing.
fn same_origin(left: &str, right: &str) -> bool {
    match (origin_of(left), origin_of(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// `scheme://host:port` of an absolute URL whose scheme has a host. `None`
/// for anything else, including a download template that is a bare path.
fn origin_of(url: &str) -> Option<String> {
    let origin = url::Url::parse(url).ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

pub(crate) fn cargo_auth_headers(config: &Config) -> Result<Arc<AuthHeaders>> {
    if is_crates_io(&config.cargo.index_url) {
        registry_auth::crates_io::<pnpm_config::Host>(&config.auth_headers, config.offline)
    } else {
        Ok(Arc::clone(&config.auth_headers))
    }
}

pub(super) async fn fetch_sparse_index(
    config: &Config,
    metadata: &str,
    http_client: &Arc<ThrottledClient>,
) -> Result<BTreeMap<String, String>> {
    let auth_headers = cargo_auth_headers(config)?;
    let cache_dir = cargo_index_cache_dir(config);
    let source = pnpm_cargo_resolver::registry_source(&config.cargo.index_url);
    let mut index_files = BTreeMap::new();

    loop {
        let missing = pnpm_cargo_resolver::missing_index_names(metadata, &index_files, &source)
            .wrap_err("discover Cargo sparse-index files")?;
        if missing.is_empty() {
            return Ok(index_files);
        }
        let fetched = stream::iter(missing)
            .map(|name| {
                let http_client = Arc::clone(http_client);
                let auth_headers = Arc::clone(&auth_headers);
                let cache_dir = cache_dir.clone();
                async move {
                    let contents = fetch_sparse_index_file(
                        &name,
                        &config.cargo.index_url,
                        &cache_dir,
                        &http_client,
                        &auth_headers,
                        config.offline,
                        config.retry_opts(),
                    )
                    .await?;
                    Ok::<_, miette::Report>((name, contents))
                }
            })
            .buffer_unordered(config.network_concurrency.clamp(1, 16))
            .try_collect::<Vec<_>>()
            .await?;
        index_files.extend(fetched);
    }
}

fn cargo_index_cache_dir(config: &Config) -> PathBuf {
    let registry = if is_crates_io(&config.cargo.index_url) {
        "crates-io".to_string()
    } else {
        pnpm_crypto_hash::create_hex_hash(config.cargo.index_url.trim_end_matches('/'))
    };
    config.cache_dir.join("v11").join("cargo-index").join(registry)
}

async fn fetch_registry_config(
    config: &Config,
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
) -> Result<RegistryConfig> {
    if is_crates_io(&config.cargo.index_url) {
        return Ok(RegistryConfig {
            dl: CRATES_IO_DOWNLOAD_BASE.to_string(),
            api: Some("https://crates.io".to_string()),
            auth_required: false,
        });
    }
    let cache_path = cargo_index_cache_dir(config).join(RegistryConfig::NAME);
    let bytes = if config.offline {
        fs::read(&cache_path).into_diagnostic().wrap_err_with(|| {
            format!("read cached Cargo registry config at {}", cache_path.display())
        })?
    } else {
        download_registry_config(config, http_client, auth_headers, &cache_path).await?
    };
    serde_json::from_slice(&bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse Cargo registry config from {}", cache_path.display()))
}

pub(super) async fn fetch_sparse_index_file(
    name: &str,
    sparse_index: &str,
    cache_dir: &Path,
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    offline: bool,
    retry_opts: RetryOpts,
) -> Result<String> {
    let relative_path = sparse_index_path(name)?;
    let cache_path = cache_dir.join(&relative_path);
    if offline {
        return fs::read_to_string(&cache_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read cached sparse index entry for {name}"));
    }

    let url = format!("{}/{relative_path}", sparse_index.trim_end_matches('/'));
    let response = http_client
        .get_bytes_with_secure_auth_and_retry(&url, auth_headers, None, retry_opts)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("fetch sparse index entry for {name}"))?;
    if !response.status.is_success() {
        return Err(miette::miette!(
            "fetch sparse index entry for {name} returned HTTP {}",
            response.status,
        ));
    }
    let contents = String::from_utf8(response.body)
        .into_diagnostic()
        .wrap_err_with(|| format!("decode sparse index entry for {name}"))?;
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .into_diagnostic()
            .wrap_err_with(|| format!("create Cargo sparse-index cache at {}", parent.display()))?;
    }
    pnpm_fs::write_atomic(&cache_path, contents.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("cache sparse index entry for {name}"))?;
    Ok(contents)
}

pub(super) fn sparse_index_path(name: &str) -> Result<String> {
    let name = name.to_ascii_lowercase();
    validate_package_field("crate name", &name, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
    })?;
    Ok(match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    })
}

async fn download_registry_config(
    config: &Config,
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    cache_path: &Path,
) -> Result<Vec<u8>> {
    let url = format!("{}/{}", config.cargo.index_url.trim_end_matches('/'), RegistryConfig::NAME);
    let response = http_client
        .get_limited_bytes_with_secure_auth_and_retry(
            &url,
            auth_headers,
            None,
            config.retry_opts(),
            MAX_REGISTRY_CONFIG_BYTES,
        )
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("fetch Cargo registry config from {url}"))?;
    if !response.status.is_success() {
        return Err(miette::miette!(
            "fetch Cargo registry config returned HTTP {}",
            response.status,
        ));
    }
    if response.body_truncated {
        return Err(miette::miette!(
            "Cargo registry config at {url} is larger than {MAX_REGISTRY_CONFIG_BYTES} bytes",
        ));
    }
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .into_diagnostic()
            .wrap_err_with(|| format!("create Cargo registry cache at {}", parent.display()))?;
    }
    pnpm_fs::write_atomic(cache_path, &response.body)
        .into_diagnostic()
        .wrap_err_with(|| format!("cache Cargo registry config at {}", cache_path.display()))?;
    Ok(response.body)
}

pub(super) async fn registry_download_config(
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<(RegistryConfig, Arc<AuthHeaders>)> {
    let cargo_auth_headers = cargo_auth_headers(config)?;
    let registry_config = fetch_registry_config(config, http_client, &cargo_auth_headers).await?;
    let auth_headers = download_auth_headers(config, &registry_config);
    Ok((registry_config, auth_headers))
}
