use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, HostNoHome, LinkProbe, NodeLinker,
    NodePackageMapType, OsString, Path, PathBuf, TrustPolicy, assert_eq, fs, io, safe_host_var,
    tempdir, write_file,
};

#[test]
pub fn materialization_env_vars_override_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "virtualStoreOnly: false\nenableModulesDir: true\n",
    )
    .expect("write to pnpm-workspace.yaml");

    struct HostWithMaterializationEnv;
    impl EnvVar for HostWithMaterializationEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_VIRTUAL_STORE_ONLY" => Some("true".to_owned()),
                "PNPM_CONFIG_ENABLE_MODULES_DIR" => Some("false".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithMaterializationEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithMaterializationEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithMaterializationEnv);
    host_current_dir!(HostWithMaterializationEnv);

    let config = Config::new().current::<HostWithMaterializationEnv>(tmp.path()).expect("loads");
    assert!(config.virtual_store_only);
    assert!(!config.enable_modules_dir);
    assert_eq!(config.hoist_pattern, Some(vec![]));
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
}

#[test]
pub fn self_update_config_ignores_a_workspace_manifest_that_raises_the_cutoff() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 4320\nminimumReleaseAgeStrict: true\n",
    )
    .expect("write to pnpm-workspace.yaml");

    let config =
        Config::new().current_for_self_update::<HostNoHome>(tmp.path()).expect("config loads");

    // A repo that raises the cutoff would pin the machine to the installed
    // pnpm, including past a release that fixes a vulnerability in it.
    assert_eq!(config.minimum_release_age, Config::new().minimum_release_age);
    assert_eq!(config.minimum_release_age_strict, None);
    assert_eq!(config.workspace_dir.as_deref(), Some(tmp.path()));
}

#[test]
pub fn self_update_config_ignores_a_workspace_manifest_that_loosens_the_cutoff() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 0\nminimumReleaseAgeStrict: false\nminimumReleaseAgeExclude:\n  - pnpm\n",
    )
    .expect("write to pnpm-workspace.yaml");

    struct HostWithStrictEnv;
    impl EnvVar for HostWithStrictEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_MINIMUM_RELEASE_AGE_STRICT" => Some("true".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithStrictEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithStrictEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithStrictEnv);
    host_current_dir!(HostWithStrictEnv);

    let config = Config::new()
        .current_for_self_update::<HostWithStrictEnv>(tmp.path())
        .expect("config loads");

    assert_eq!(config.minimum_release_age, Config::new().minimum_release_age);
    assert_eq!(config.minimum_release_age_strict, Some(true));
    assert_eq!(config.minimum_release_age_exclude, None);
}

#[test]
pub fn self_update_config_ignores_a_workspace_manifest_that_loosens_the_trust_policy() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "trustPolicy: off\ntrustPolicyExclude:\n  - pnpm\ntrustPolicyIgnoreAfter: 525600\n",
    )
    .expect("write to pnpm-workspace.yaml");

    struct HostWithTrustPolicyEnv;
    impl EnvVar for HostWithTrustPolicyEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_TRUST_POLICY" => Some("no-downgrade".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithTrustPolicyEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithTrustPolicyEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithTrustPolicyEnv);
    host_current_dir!(HostWithTrustPolicyEnv);

    let config = Config::new()
        .current_for_self_update::<HostWithTrustPolicyEnv>(tmp.path())
        .expect("config loads");

    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude, None);
    assert_eq!(config.trust_policy_ignore_after, None);
}

#[test]
pub fn self_update_config_keeps_non_policy_workspace_settings() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write to pnpm-workspace.yaml");

    let config =
        Config::new().current_for_self_update::<HostNoHome>(tmp.path()).expect("config loads");

    assert_eq!(config.node_linker, NodeLinker::Hoisted);
}

#[test]
pub fn workspace_manifest_still_sets_the_release_age_policy_for_other_commands() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "minimumReleaseAge: 4320\n")
        .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("config loads");

    assert_eq!(config.minimum_release_age, Some(4320));
}

/// `PNPM_CONFIG_HOIST=false` runs the same post-processing as
/// yaml-set `hoist: false` — it short-circuits `hoist_pattern`
/// to `None`, following the rule that `hoist: false` clears the
/// hoist pattern. Without this, the install-time
/// `hoist_pattern.is_some() || public_hoist_pattern.is_some()` guard
/// would still enable hoisting even after the user disabled it via
/// env var.
#[test]
pub fn pnpm_config_hoist_false_clears_hoist_pattern() {
    struct HostWithHoistEnv;
    impl EnvVar for HostWithHoistEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_HOIST" {
                return Some("false".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithHoistEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithHoistEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithHoistEnv);
    host_current_dir!(HostWithHoistEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithHoistEnv>(tmp.path()).expect("loads");
    assert!(!config.hoist);
    assert_eq!(
        config.hoist_pattern, None,
        "hoist: false must clear hoist_pattern, even when set via env var",
    );
}

#[test]
pub fn shamefully_hoist_derives_the_public_hoist_pattern() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "shamefullyHoist: true\npublicHoistPattern:\n  - eslint\n",
    )
    .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");

    assert_eq!(config.public_hoist_pattern, Some(vec!["*".to_string()]));
}

#[test]
pub fn shamefully_hoist_false_disables_public_hoisting() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "shamefullyHoist: false\npublicHoistPattern:\n  - eslint\n",
    )
    .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");

    assert_eq!(config.public_hoist_pattern, None);
}

#[test]
pub fn unset_shamefully_hoist_preserves_the_public_hoist_pattern() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "publicHoistPattern:\n  - eslint\n")
        .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");

    assert_eq!(config.public_hoist_pattern, Some(vec!["eslint".to_string()]));
}

#[test]
pub fn virtual_store_dir_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "virtualStoreDirMaxLength: 90\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.virtual_store_dir_max_length, 90);
}

#[test]
pub fn engine_strict_node_version_and_max_sockets_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "engineStrict: true\nnodeVersion: 18.20.4\nmaxSockets: 5\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.engine_strict);
    assert_eq!(config.node_version.as_deref(), Some("18.20.4"));
    assert_eq!(config.max_sockets, Some(5));
}

/// The environment is a later layer than the file, so npm's spelling there
/// still wins over the canonical spelling in `pnpm-workspace.yaml`.
#[test]
pub fn max_sockets_from_the_environment_wins_over_the_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "maxSockets: 4\n")
        .expect("write to pnpm-workspace.yaml");

    set_fake_env(&[("PNPM_CONFIG_MAXSOCKETS", "8")]);
    assert_eq!(load_with_fake_env(tmp.path()).max_sockets, Some(8));
}

#[test]
pub fn update_notifier_and_legacy_dir_filtering_default_and_come_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(config.update_notifier);
    assert!(!config.legacy_dir_filtering);

    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "updateNotifier: false\nlegacyDirFiltering: true\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(!config.update_notifier);
    assert!(config.legacy_dir_filtering);
}

#[test]
pub fn init_settings_come_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.init_author_name, None);
    assert_eq!(config.init_license, None);
    assert_eq!(config.init_version, None);

    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "initAuthorName: pnpm\ninitAuthorEmail: xxxxxx@pnpm.com\ninitAuthorUrl: https://www.github.com/pnpm\ninitLicense: MIT\ninitVersion: 2.0.0\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.init_author_name.as_deref(), Some("pnpm"));
    assert_eq!(config.init_author_email.as_deref(), Some("xxxxxx@pnpm.com"));
    assert_eq!(config.init_author_url.as_deref(), Some("https://www.github.com/pnpm"));
    assert_eq!(config.init_license.as_deref(), Some("MIT"));
    assert_eq!(config.init_version.as_deref(), Some("2.0.0"));
}

#[test]
pub fn node_version_from_pnpm_config_env_overrides_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "nodeVersion: 18.20.4\n")
        .expect("write to pnpm-workspace.yaml");

    set_fake_env(&[("PNPM_CONFIG_NODE_VERSION", "20.0.0")]);
    let config = load_with_fake_env(tmp.path());

    assert_eq!(config.node_version.as_deref(), Some("20.0.0"));
}

#[test]
pub fn catalog_prune_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.catalog_prune);
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "catalogPrune: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.catalog_prune);
}

/// `catalogPrune` was released as `cleanupUnusedCatalogs`, which every
/// `pnpm-workspace.yaml` written since pnpm 10.15 may still carry.
#[test]
pub fn catalog_prune_from_its_former_name_in_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "cleanupUnusedCatalogs: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.catalog_prune);
}

/// The canonical name wins, whichever order the two appear in.
#[test]
pub fn catalog_prune_overrides_its_former_name() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "cleanupUnusedCatalogs: true\ncatalogPrune: false\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(!config.catalog_prune);
}

#[test]
pub fn minimum_release_age_exclude_prune_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.minimum_release_age_exclude_prune);
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "minimumReleaseAgeExcludePrune: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.minimum_release_age_exclude_prune);
}

#[test]
pub fn trust_policy_exclude_prune_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.trust_policy_exclude_prune);
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "trustPolicyExcludePrune: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.trust_policy_exclude_prune);
}

#[test]
pub fn runtime_on_fail_from_workspace_yaml() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "runtimeOnFail: download\n").unwrap();
    let config = Config::default().current::<Host>(dir.path()).unwrap();
    assert_eq!(config.runtime_on_fail, Some(crate::RuntimeOnFail::Download));
}

/// `lockfileDir` moves the paths pnpm resolves against it: the root
/// `node_modules` and the virtual store follow the pin, and the config
/// root the install reads its root manifest and pnpmfile from becomes the
/// pin rather than the workspace root.
#[test]
pub fn lockfile_dir_from_workspace_yaml_moves_the_paths_anchored_on_it() {
    let tmp = tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    fs::create_dir(&workspace).expect("create the workspace dir");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "lockfileDir: ..
",
    )
    .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(&workspace).expect("yaml is valid");

    assert_eq!(config.lockfile_dir.as_deref(), Some(tmp.path()));
    assert_eq!(config.lockfile_dir_for(&workspace), tmp.path());
    assert_eq!(config.root_project_manifest_dir(&workspace), tmp.path());
    assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules").join(".pnpm"));
}

#[test]
pub fn package_map_settings_load_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "nodeExperimentalPackageMap: true\nnodePackageMapType: loose\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.node_experimental_package_map);
    assert_eq!(config.node_package_map_type, NodePackageMapType::Loose);
}

#[test]
pub fn peers_suffix_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "peersSuffixMaxLength: 10\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.peers_suffix_max_length, 10);
}

/// A repository must not reach strict mode for `self-update`: turning it on
/// would let the repo refuse an immature pnpm release and pin the machine to
/// the installed version, which is what
/// [`WorkspaceSettings::clear_self_update_policy`] exists to prevent.
#[test]
fn self_update_ignores_a_workspace_release_age_for_strict_mode() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "minimumReleaseAge: 4320\n")
        .expect("write to pnpm-workspace.yaml");

    let config =
        Config::new().current_for_self_update::<HostNoHome>(tmp.path()).expect("config loads");

    assert!(!config.resolved_minimum_release_age_strict());
}

/// A `pnpm-workspace.yaml` `registries:` block is repository-controlled, so
/// it must not steer package-manager bootstrap either — neither the default
/// nor a scoped route. Regression test for GHSA-j2hc-m6cf-6jm8.
#[test]
pub fn package_manager_bootstrap_ignores_workspace_yaml_registries() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "registries:\n  default: https://attacker.example.com/\n  '@evil': https://attacker-scoped.example.com/\n",
    )
    .expect("write pnpm-workspace.yaml");
    let config = Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config");

    assert_eq!(
        config.registry, "https://attacker.example.com/",
        "workspace yaml drives normal installs",
    );
    assert_eq!(
        config.registries_by_scope.get("@evil").map(String::as_str),
        Some("https://attacker-scoped.example.com/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://trusted.example.com/",
        "package-manager bootstrap ignores the workspace yaml default registry",
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@evil"),
        None,
        "package-manager bootstrap ignores workspace yaml scoped registries",
    );
}

// Port of `extraBinPaths` in `config/reader/test/index.ts` — empty outside
// a workspace; exactly the workspace root's `node_modules/.bin` inside one.
#[test]
pub fn extra_bin_paths_lists_workspace_root_bin_only_inside_a_workspace() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());
    assert_eq!(config.extra_bin_paths, Vec::<PathBuf>::new());

    fs::write(project.path().join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");
    let config = load_with_fake_env(project.path());
    assert_eq!(config.extra_bin_paths, vec![project.path().join("node_modules").join(".bin")]);
}
