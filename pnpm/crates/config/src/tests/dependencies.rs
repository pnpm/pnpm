use super::{Config, HostNoHome, assert_eq, tempdir};

#[test]
pub fn engine_strict_node_version_and_max_sockets_default() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.engine_strict);
    assert_eq!(config.node_version, None);
    assert_eq!(config.max_sockets, None);
}

#[test]
pub fn force_ignores_platform_defaults_to_false() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.force_ignores_platform);
    assert!(!config.installs_incompatible_packages());
}

#[test]
pub fn force_lifts_engine_strict_and_installs_incompatible_packages_only_when_opted_in() {
    let tmp = tempdir().unwrap();
    let mut config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    config.engine_strict = true;
    config.force = true;
    assert!(!config.effective_engine_strict());
    assert!(!config.installs_incompatible_packages());
    config.force_ignores_platform = true;
    assert!(config.installs_incompatible_packages());
    config.force = false;
    assert!(config.effective_engine_strict());
    assert!(!config.installs_incompatible_packages());
}

#[test]
pub fn peers_suffix_max_length_defaults_to_1000() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 1000);
}
