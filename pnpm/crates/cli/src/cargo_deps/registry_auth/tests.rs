use super::{crates_io, crates_io_from_sources, token_from_credentials};
use pnpm_config::{EnvVar, EnvVarOs, GetHomeDir};
use pnpm_network::AuthHeaders;
use std::{ffi::OsString, fs, path::PathBuf, sync::Arc};

struct NoCredentialAccess;

impl EnvVar for NoCredentialAccess {
    fn var(_name: &str) -> Option<String> {
        panic!("offline auth resolution must not read environment variables")
    }
}

impl EnvVarOs for NoCredentialAccess {
    fn var_os(_name: &str) -> Option<OsString> {
        panic!("offline auth resolution must not read environment variables")
    }
}

impl GetHomeDir for NoCredentialAccess {
    fn home_dir() -> Option<PathBuf> {
        panic!("offline auth resolution must not inspect the home directory")
    }
}

fn configured() -> Arc<AuthHeaders> {
    Arc::new(AuthHeaders::from_creds_map([
        ("//index.crates.io/".to_string(), "Bearer pnpm-token".to_string()),
        ("//npm.example/".to_string(), "Bearer npm-token".to_string()),
    ]))
}

#[test]
fn missing_credentials_files_reuse_the_configured_auth_map() {
    let configured = configured();
    let cargo_home = tempfile::tempdir().unwrap();

    let resolved = crates_io_from_sources(&configured, None, Some(cargo_home.path())).unwrap();

    assert!(Arc::ptr_eq(&resolved, &configured));
    assert_eq!(
        resolved.for_url("https://index.crates.io/config.json"),
        Some("Bearer pnpm-token".to_string()),
    );
}

#[test]
fn offline_mode_does_not_read_cargo_credentials() {
    let configured = configured();

    let resolved = crates_io::<NoCredentialAccess>(&configured, true).unwrap();

    assert!(Arc::ptr_eq(&resolved, &configured));
}

#[test]
fn credentials_token_is_bare_and_preserves_other_routes() {
    let cargo_home = tempfile::tempdir().unwrap();
    fs::write(cargo_home.path().join("credentials.toml"), "[registry]\ntoken = 'cargo-token'\n")
        .unwrap();

    let resolved = crates_io_from_sources(&configured(), None, Some(cargo_home.path())).unwrap();

    assert_eq!(
        resolved.for_url("https://index.crates.io/se/rd/serde"),
        Some("cargo-token".to_string()),
    );
    assert_eq!(resolved.for_url("https://static.crates.io/crates/serde/serde-1.0.0.crate"), None);
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
    fs::write(cargo_home.path().join("credentials.toml"), "[registry]\ntoken = 'toml'\n").unwrap();

    let resolved = crates_io_from_sources(&configured(), None, Some(cargo_home.path())).unwrap();

    assert_eq!(resolved.for_url("https://index.crates.io/config.json"), Some("legacy".to_string()));
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
