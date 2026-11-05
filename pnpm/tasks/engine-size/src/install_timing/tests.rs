use super::set_setting;
use pretty_assertions::assert_eq;

#[test]
fn set_setting_appends_a_missing_key() {
    let mut settings = "packages:\n  - a\n".to_string();
    set_setting(&mut settings, "ignoreScripts: true");
    assert_eq!(settings, "packages:\n  - a\nignoreScripts: true\n");
}

#[test]
fn set_setting_replaces_an_existing_entry_and_its_continuation_lines() {
    let mut settings =
        "storeDir: /shared/store\nignoreScripts:\n  false\nregistries:\n  default: x\n".to_string();
    set_setting(&mut settings, "ignoreScripts: true");
    set_setting(&mut settings, "storeDir: ./store-dir");
    assert_eq!(settings, "registries:\n  default: x\nignoreScripts: true\nstoreDir: ./store-dir\n");
}

#[test]
fn set_setting_leaves_keys_that_share_a_prefix() {
    let mut settings = "storeDirectory: x\n".to_string();
    set_setting(&mut settings, "storeDir: ./store-dir");
    assert_eq!(settings, "storeDirectory: x\nstoreDir: ./store-dir\n");
}
