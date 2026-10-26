use super::{Config, compute_upstream_cache_namespace};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;

fn config_from(yaml: &str) -> Config {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("pnpr.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::from_yaml(&path, listen, None).unwrap();
    config.resolution_cache_secret = b"fixed-secret".to_vec().into();
    config
}

#[test]
fn upstream_cache_namespace_is_stable_across_ecosystem_grouping() {
    let top_level = config_from(
        "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  private-npm:
    type: upstream
    url: https://npm.example.com/
    access: $authenticated
  crates:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
",
    );
    let grouped = config_from(
        "\
storage: ./s
registries:
  npm:
    npmjs:
      type: upstream
      url: https://registry.npmjs.org/
      public: true
    private-npm:
      type: upstream
      url: https://npm.example.com/
      access: $authenticated
  cargo:
    crates:
      type: upstream
      url: https://index.crates.io/
      public: true
",
    );

    assert_eq!(
        compute_upstream_cache_namespace(&top_level, "npmjs"),
        compute_upstream_cache_namespace(&grouped, "npm/npmjs"),
    );
    assert_eq!(
        compute_upstream_cache_namespace(&top_level, "private-npm"),
        compute_upstream_cache_namespace(&grouped, "npm/private-npm"),
    );
    assert_eq!(
        compute_upstream_cache_namespace(&top_level, "crates"),
        compute_upstream_cache_namespace(&grouped, "cargo/crates"),
    );
}
