use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::{EnvVar, EnvVarOs, GetHomeDir};
use pnpm_network::AuthHeaders;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const CARGO_HOME_ENV: &str = "CARGO_HOME";
const CRATES_IO_TOKEN_ENV: &str = "CARGO_REGISTRY_TOKEN";

#[derive(Deserialize)]
struct CargoCredentials {
    registry: Option<RegistryCredentials>,
}

#[derive(Deserialize)]
struct RegistryCredentials {
    token: Option<String>,
}

pub(super) fn crates_io<Sys>(
    configured: &Arc<AuthHeaders>,
    offline: bool,
) -> Result<Arc<AuthHeaders>>
where
    Sys: EnvVar + EnvVarOs + GetHomeDir,
{
    if offline {
        return Ok(Arc::clone(configured));
    }
    let cargo_home = cargo_home::<Sys>();
    crates_io_from_sources(configured, Sys::var(CRATES_IO_TOKEN_ENV), cargo_home.as_deref())
}

fn crates_io_from_sources(
    configured: &Arc<AuthHeaders>,
    env_token: Option<String>,
    cargo_home: Option<&Path>,
) -> Result<Arc<AuthHeaders>> {
    let token = match nonempty(env_token) {
        Some(token) => Some(token),
        None => cargo_home.map(token_from_credentials).transpose()?.flatten(),
    };
    let Some(token) = token else { return Ok(Arc::clone(configured)) };

    let mut auth_headers = (**configured).clone();
    auth_headers.insert_url_header(pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX, token);
    Ok(Arc::new(auth_headers))
}

fn cargo_home<Sys>() -> Option<PathBuf>
where
    Sys: EnvVarOs + GetHomeDir,
{
    Sys::var_os(CARGO_HOME_ENV)
        .filter(|path| !path.as_os_str().is_empty())
        .map(PathBuf::from)
        .or_else(|| Sys::home_dir().map(|home| home.join(".cargo")))
}

fn token_from_credentials(cargo_home: &Path) -> Result<Option<String>> {
    for filename in ["credentials", "credentials.toml"] {
        let path = cargo_home.join(filename);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("read Cargo credentials from {}", path.display()));
            }
        };
        let parse_error = format!("parse Cargo credentials from {}", path.display());
        let credentials: CargoCredentials =
            toml::from_str(&contents).map_err(|_| miette::miette!(parse_error))?;
        return Ok(credentials.registry.and_then(|registry| nonempty(registry.token)));
    }
    Ok(None)
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
