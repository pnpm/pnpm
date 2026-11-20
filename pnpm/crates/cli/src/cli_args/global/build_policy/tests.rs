use super::legacy_settings_warnings;

#[test]
fn legacy_settings_warnings_point_only_supported_settings_to_global_packages_config() {
    let warnings = legacy_settings_warnings(
        "/global/pnpm-workspace.yaml",
        "/config/config.yaml",
        &["allowBuilds", "overrides", "trustPolicyExclude", "packageExtensions"],
    );

    assert_eq!(
        warnings,
        [
            "The build settings in /global/pnpm-workspace.yaml are deprecated. Move them under globalPackagesConfig in /config/config.yaml.",
            "These settings in /global/pnpm-workspace.yaml do not apply to global packages: trustPolicyExclude. Set them under globalPackagesConfig in /config/config.yaml.",
            "These settings in /global/pnpm-workspace.yaml are not supported for global packages and are ignored: overrides, packageExtensions.",
        ],
    );
}
