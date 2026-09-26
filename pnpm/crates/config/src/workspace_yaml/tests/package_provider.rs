use super::{Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn apply_resolves_path_shaped_package_provider_against_base_dir() {
    let base = Path::new("/workspace/root");

    let settings: WorkspaceSettings =
        serde_saphyr::from_str("packageProvider: ./provider/run.js\n").unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, base);
    assert_eq!(
        config.package_provider.as_deref(),
        Some(
            base.join("./provider/run.js")
                .to_string_lossy()
                .as_ref()
        ),
    );

    let settings: WorkspaceSettings =
        serde_saphyr::from_str("packageProvider: pnpm-nix-provider\n").unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, base);
    assert_eq!(config.package_provider.as_deref(), Some("pnpm-nix-provider"));
}
