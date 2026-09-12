use super::configured_node_options;
use pnpm_config::Config;

#[test]
fn configured_node_options_preserves_extra_env_without_a_node_options_setting() {
    let mut config = Config::default();
    config.extra_env.insert("NODE_OPTIONS".to_string(), "--trace-warnings".to_string());

    assert_eq!(configured_node_options(&config).as_deref(), Some("--trace-warnings"));
}
