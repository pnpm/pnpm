use super::runtime_mock_config;
use std::fs;
use tempfile::TempDir;

#[test]
fn mock_config_resolves_htpasswd_from_the_runtime_config_directory() {
    let home = TempDir::new().expect("create runtime storage");
    let config_dir = home.path().join(".registry-mock");
    fs::create_dir_all(&config_dir).expect("create runtime config directory");
    fs::write(config_dir.join("htpasswd"), "benchmark-user:hash\n").expect("write benchmark user");

    let config = runtime_mock_config(&home.path().join(".metadata/metadata.sqlite"), &config_dir)
        .expect("write mock config");

    assert_eq!(config.parent(), Some(config_dir.as_path()));
    let yaml = fs::read_to_string(config).expect("read mock config");
    assert!(yaml.contains("file: ./htpasswd"));
}
