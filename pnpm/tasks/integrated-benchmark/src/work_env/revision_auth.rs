use super::{
    PNPR_BENCHMARK_HTPASSWD, PNPR_BENCHMARK_USERNAME,
    registries_home::{fixture_registries, revision_source},
};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[cfg(test)]
mod tests;

/// Historical binaries read their own fixture schema and isolated credentials.
pub(super) fn mock_config(binary: &Path, home: &Path) -> String {
    let yaml = fs::read_to_string(revision_source(binary).join("pnpr/.fixtures/config.yaml"))
        .expect("read the revision mock configuration");
    let mut config: Value =
        serde_saphyr::from_str(&yaml).expect("parse revision mock configuration");
    config["auth"] = seed_auth(&yaml, home);
    let database_key = if config.get("database").is_some() { "database" } else { "metadata" };
    let database = std::path::absolute(home)
        .expect("absolute revision database directory")
        .join("metadata.sqlite");
    config[database_key] = json!({"url": format!("sqlite:{}", database.display())});
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
    let Some(users) = config["auth"].get("users") else {
        let path = home.join("htpasswd");
        fs::write(&path, PNPR_BENCHMARK_HTPASSWD).expect("seed revision htpasswd account");
        return json!({"htpasswd": {"file": path, "maxUsers": -1}});
    };
    if config["auth"].get("tokens").is_none() {
        // Accounts live in the database, so the benchmark login
        // registers the account the first time it runs.
        let mut users = users.clone();
        users["maxUsers"] = json!(1);
        return json!({"users": users});
    }
    let path = home.join("users.json");
    let (_, hash) = PNPR_BENCHMARK_HTPASSWD
        .trim()
        .split_once(':')
        .expect("benchmark credential");
    let account = json!({
        "id": "benchmark-account",
        "status": "active",
        "credential_kind": "bcrypt",
        "password_hash": hash,
        "created_at": 0,
    });
    let accounts = json!({PNPR_BENCHMARK_USERNAME: account});
    fs::write(&path, accounts.to_string()).expect("seed revision JSON account");
    json!({"users": {"file": path, "maxUsers": -1}})
}

pub(super) fn mock_command(
    binary: &Path,
    port: u16,
    public_url: Option<&str>,
    home: &Path,
) -> std::process::Command {
    let yaml = mock_config(binary, home);
    let registries = fixture_registries(revision_source(binary));
    pnpm_registry_mock::pnpr_command_with_home(
        binary,
        port,
        public_url,
        Some(pnpm_registry_mock::PnprMockConfig {
            home,
            yaml: &yaml,
            registries: registries.as_deref(),
        }),
    )
}
