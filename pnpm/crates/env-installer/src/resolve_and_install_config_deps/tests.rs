use super::npm_lockfile_form;
use pnpm_lockfile::{LockfileResolution, TarballResolution};
use std::collections::BTreeMap;

#[test]
fn pnpr_config_dependency_uses_the_declared_tarball_layout() {
    let registry = "https://registry.example/";
    let registry_options_by_url = BTreeMap::from([(
        registry.to_string(),
        pnpm_lockfile::RegistryOptions {
            server_type: Some(pnpm_lockfile::RegistryServerType::Pnpr),
            ..Default::default()
        },
    )]);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{registry}@pnpm.e2e/foo@1.0.0"),
        integrity: Some(ssri::Integrity::from(b"foo")),
        revision: None,
        git_hosted: None,
        path: None,
    });

    let lockfile_resolution = resolution
        .to_lockfile_form(
            "@pnpm.e2e/foo",
            "1.0.0",
            npm_lockfile_form(registry, &registry_options_by_url),
        )
        .unwrap();

    assert!(matches!(lockfile_resolution, LockfileResolution::Registry(_)));
}
