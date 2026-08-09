use super::is_pnpm_config_var;

#[test]
fn pnpm_and_npm_settings_match_in_either_spelling() {
    for name in [
        "PNPM_CONFIG_GLOBAL_SHIMS",
        "pnpm_config_global_shims",
        "PnPm_CoNfIg_GlObAl_ShImS",
        "NPM_CONFIG_REGISTRY",
        "npm_config_registry",
        "PNPM_SHIM_BYPASS",
        "pnpm_shim_bypass",
    ] {
        assert!(is_pnpm_config_var(name), "{name} should be stripped");
    }
}

/// The suites locate the ambient pnpm they spawn for compatibility checks
/// through the environment, and a bare prefix names no setting at all.
#[test]
fn the_environment_the_suites_rely_on_is_left_alone() {
    for name in ["PNPM_HOME", "PATH", "HOME", "PNPM_CONFIG_", "npm_config_", "PNPM_SHIM_BYPASS_X"] {
        assert!(!is_pnpm_config_var(name), "{name} should be kept");
    }
}
