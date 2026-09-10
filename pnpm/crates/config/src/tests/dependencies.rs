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
pub fn peers_suffix_max_length_defaults_to_1000() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 1000);
}
