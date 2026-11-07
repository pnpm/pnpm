use super::{PNPR_BENCHMARK_HTPASSWD, PNPR_BENCHMARK_USERNAME, mock_config, seed_auth};
use serde_json::Value;
use std::fs;

#[test]
fn revision_accounts_use_the_revisions_format_and_separate_files() {
    let directory = tempfile::tempdir().unwrap();
    let old_home = directory.path().join("old");
    let new_home = directory.path().join("new");
    let old = seed_auth("auth:\n  htpasswd:\n    maxUsers: -1\n", &old_home);
    let new = seed_auth("auth:\n  users:\n    maxUsers: -1\n", &new_home);
    assert_eq!(
        old["htpasswd"]["file"],
        old_home
            .join("auth")
            .join("htpasswd")
            .to_str()
            .unwrap(),
    );
    assert_eq!(
        new["users"]["file"],
        new_home
            .join("auth")
            .join("users.json")
            .to_str()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(old_home.join("auth").join("htpasswd")).unwrap(),
        PNPR_BENCHMARK_HTPASSWD,
    );
    let accounts: Value =
        serde_json::from_slice(&fs::read(new_home.join("auth").join("users.json")).unwrap())
            .unwrap();
    assert_eq!(accounts[PNPR_BENCHMARK_USERNAME]["id"], "benchmark-account");
    let hash = accounts[PNPR_BENCHMARK_USERNAME]["password_hash"].as_str().unwrap();
    assert_eq!(format!("{PNPR_BENCHMARK_USERNAME}:{hash}\n"), PNPR_BENCHMARK_HTPASSWD);
    assert!(
        !old_home
            .join("auth")
            .join("users.json")
            .exists(),
    );
    assert!(
        !new_home
            .join("auth")
            .join("htpasswd")
            .exists(),
    );
}

#[test]
fn mock_config_comes_from_the_binarys_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("revision");
    fs::create_dir_all(source.join("pnpr/.fixtures")).unwrap();
    fs::write(
        source.join("pnpr/.fixtures/config.yaml"),
        "auth:\n  htpasswd: {}\nsecret: revision-secret\n",
    )
    .unwrap();
    let home = directory.path().join("mock-home");
    let yaml = mock_config(&source.join("target/release/pnpr"), &home);
    let config: Value = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(config["secret"], "revision-secret");
    assert_eq!(
        config["auth"]["htpasswd"]["file"],
        home.join("auth")
            .join("htpasswd")
            .to_str()
            .unwrap(),
    );
    assert!(config["auth"].get("users").is_none());
}

#[test]
fn seeding_a_moving_revision_discards_old_auth_state() {
    let directory = tempfile::tempdir().unwrap();
    let source = "auth:\n  users: {}\n";
    seed_auth(source, directory.path());
    let tokens = directory
        .path()
        .join("auth")
        .join("tokens.db");
    fs::write(&tokens, "incompatible schema").unwrap();
    seed_auth(source, directory.path());
    assert!(!tokens.exists());
}
