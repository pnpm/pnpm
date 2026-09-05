use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::{EnvVar, EnvVarOs, GetHomeDir};
use pnpm_network::AuthHeaders;
use serde::Deserialize;
use std::{fs, path::Path, path::PathBuf, sync::Arc};

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

pub(super) fn crates_io<Sys>(configured: &Arc<AuthHeaders>) -> Result<Arc<AuthHeaders>>
where
    Sys: EnvVar + EnvVarOs + GetHomeDir,
{
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
    auth_headers.insert_url_header(super::CRATES_IO_SPARSE_INDEX, token);
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
        let credentials: CargoCredentials = toml::from_str(&contents)
            .map_err(|_| miette::miette!("parse Cargo credentials from {}", path.display()))?;
        return Ok(credentials.registry.and_then(|registry| nonempty(registry.token)));
    }
    Ok(None)
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> Arc<AuthHeaders> {
        Arc::new(AuthHeaders::from_creds_map([
            ("//index.crates.io/".to_string(), "Bearer pnpm-token".to_string()),
            ("//npm.example/".to_string(), "Bearer npm-token".to_string()),
        ]))
    }

    #[test]
    fn no_cargo_token_reuses_the_configured_auth_map() {
        let configured = configured();

        let resolved = crates_io_from_sources(&configured, None, None).unwrap();

        assert!(Arc::ptr_eq(&resolved, &configured));
        assert_eq!(
            resolved.for_url("https://index.crates.io/config.json"),
            Some("Bearer pnpm-token".to_string()),
        );
    }

    #[test]
    fn credentials_token_is_bare_and_preserves_other_routes() {
        let cargo_home = tempfile::tempdir().unwrap();
        fs::write(
            cargo_home.path().join("credentials.toml"),
            "[registry]\ntoken = 'cargo-token'\n",
        )
        .unwrap();

        let resolved =
            crates_io_from_sources(&configured(), None, Some(cargo_home.path())).unwrap();

        assert_eq!(
            resolved.for_url("https://index.crates.io/se/rd/serde"),
            Some("cargo-token".to_string()),
        );
        assert_eq!(
            resolved.for_url("https://static.crates.io/crates/serde/serde-1.0.0.crate"),
            None
        );
        assert_eq!(
            resolved.for_url("https://npm.example/package"),
            Some("Bearer npm-token".to_string()),
        );
    }

    #[test]
    fn environment_token_overrides_the_credentials_file() {
        let cargo_home = tempfile::tempdir().unwrap();
        fs::write(cargo_home.path().join("credentials.toml"), "not valid TOML = [").unwrap();

        let resolved = crates_io_from_sources(
            &configured(),
            Some("environment-token".to_string()),
            Some(cargo_home.path()),
        )
        .unwrap();

        assert_eq!(
            resolved.for_url("https://index.crates.io/config.json"),
            Some("environment-token".to_string()),
        );
    }

    #[test]
    fn legacy_credentials_file_wins_when_both_exist() {
        let cargo_home = tempfile::tempdir().unwrap();
        fs::write(cargo_home.path().join("credentials"), "[registry]\ntoken = 'legacy'\n").unwrap();
        fs::write(cargo_home.path().join("credentials.toml"), "[registry]\ntoken = 'toml'\n")
            .unwrap();

        let resolved =
            crates_io_from_sources(&configured(), None, Some(cargo_home.path())).unwrap();

        assert_eq!(
            resolved.for_url("https://index.crates.io/config.json"),
            Some("legacy".to_string()),
        );
    }

    #[test]
    fn malformed_credentials_do_not_leak_the_file_contents() {
        let cargo_home = tempfile::tempdir().unwrap();
        fs::write(
            cargo_home.path().join("credentials.toml"),
            "[registry]\ntoken = 'do-not-print-this'\ninvalid = [",
        )
        .unwrap();

        let error = token_from_credentials(cargo_home.path()).unwrap_err().to_string();

        assert!(error.contains("credentials.toml"), "{error}");
        assert!(!error.contains("do-not-print-this"), "{error}");
    }
}
