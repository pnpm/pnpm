use super::{PNPR_BENCHMARK_HTPASSWD, PNPR_BENCHMARK_USERNAME};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[cfg(test)]
mod tests;

/// Historical binaries read their own fixture schema and isolated credentials.
pub(super) fn mock_config(binary: &Path, home: &Path) -> String {
    let source = binary
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("revision binary is under target/release");
    let yaml = fs::read_to_string(source.join("pnpr/.fixtures/config.yaml"))
        .expect("read the revision mock configuration");
    let mut config: Value =
        serde_saphyr::from_str(&yaml).expect("parse revision mock configuration");
    config["auth"] = seed_auth(&yaml, home);
    serde_saphyr::to_string(&config).expect("serialize revision mock configuration")
}

pub(super) fn seed_auth(source: &str, home: &Path) -> Value {
    let config: Value = serde_saphyr::from_str(source).expect("parse revision auth configuration");
    let home = std::path::absolute(home).expect("absolute revision auth directory").join("auth");
    match fs::remove_dir_all(&home) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!("remove stale revision auth state: {err}"),
    }
    fs::create_dir_all(&home).expect("create revision auth directory");
    if config["auth"].get("users").is_some() {
        let path = home.join("users.json");
        let (_, hash) = PNPR_BENCHMARK_HTPASSWD
            .trim()
            .split_once(':')
            .expect("benchmark credential");
        let accounts =
            json!({PNPR_BENCHMARK_USERNAME: {"id": "benchmark-account", "password_hash": hash}});
        fs::write(&path, accounts.to_string()).expect("seed revision JSON account");
        json!({"users": {"file": path, "maxUsers": -1}})
    } else {
        let path = home.join("htpasswd");
        fs::write(&path, PNPR_BENCHMARK_HTPASSWD).expect("seed revision htpasswd account");
        json!({"htpasswd": {"file": path, "maxUsers": -1}})
    }
}

pub(super) fn mock_command(
    binary: &Path,
    port: u16,
    public_url: Option<&str>,
    home: &Path,
) -> std::process::Command {
    let yaml = mock_config(binary, home);
    pnpm_registry_mock::pnpr_command_with_home(
        binary,
        port,
        public_url,
        Some(pnpm_registry_mock::PnprMockConfig { home, yaml: &yaml }),
    )
}
