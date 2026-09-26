use super::opted_in;
use pnpm_config::Config;

#[test]
fn workflow_files_are_read_only_when_opted_in() {
    let mut config = Config::new();
    assert!(!opted_in(false, &config));
    assert!(opted_in(true, &config));

    config.update_config.github_actions = Some(false);
    assert!(!opted_in(false, &config));
    assert!(opted_in(true, &config));

    config.update_config.github_actions = Some(true);
    assert!(opted_in(false, &config));
}
