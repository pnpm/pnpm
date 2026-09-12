use super::{
    CatalogMode, Config, EnvVar, HoistingLimits, LinkWorkspacePackages, Path, RegistryEntry,
    StoreDir, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq,
};

/// `scope` is a valid key in the global `config.yaml` (pnpm's
/// `isConfigFileKey` accepts it), so it must survive the workspace-only
/// stripping that runs on that file.
#[test]
fn scope_survives_workspace_only_field_clearing() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str("scope: '@from-global'\n").unwrap();
    settings.clear_workspace_only_fields();
    assert_eq!(settings.scope.as_deref(), Some("@from-global"));
}

/// The workspace-structural keys are parsed so `pnpm config get` / `list`
/// can show them, but only from the workspace yaml: the global `config.yaml`
/// refuses them.
#[test]
fn structural_keys_are_parsed_and_are_workspace_only() {
    let yaml = "
packages: ['.']
catalog:
  react: ^19.0.0
catalogs:
  react17:
    react: ^17.0.0
onlyBuiltDependencies: [esbuild]
neverBuiltDependencies: [fsevents]
ignoredBuiltDependencies: [core-js]
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.packages.as_deref(), Some(&[".".to_owned()][..]));
    assert_eq!(
        settings.catalog.as_ref().and_then(|c| c.get("react")).map(String::as_str),
        Some("^19.0.0"),
    );
    assert_eq!(
        settings
            .catalogs
            .as_ref()
            .and_then(|c| c.get("react17"))
            .and_then(|c| c.get("react"))
            .map(String::as_str),
        Some("^17.0.0"),
    );
    assert_eq!(settings.only_built_dependencies.as_deref(), Some(&["esbuild".to_owned()][..]));
    assert_eq!(settings.never_built_dependencies.as_deref(), Some(&["fsevents".to_owned()][..]));
    assert_eq!(settings.ignored_built_dependencies.as_deref(), Some(&["core-js".to_owned()][..]));

    settings.clear_workspace_only_fields();
    assert_eq!(settings, WorkspaceSettings::default());
}

/// Env-var placeholders inside workspace request destinations are ignored so
/// repository-controlled config cannot smuggle victim environment
/// values into outbound requests.
#[test]
fn ignores_env_vars_inside_workspace_request_destination_values() {
    struct EnvWithHost;
    impl EnvVar for EnvWithHost {
        fn var(name: &str) -> Option<String> {
            (name == "WORK_HOST").then(|| "internal.example.com".to_owned())
        }
    }

    let yaml = r"
pnprServer: https://${WORK_HOST}/pnpr/
registry: https://${WORK_HOST}/npm/
httpsProxy: http://${WORK_HOST}:8080/
httpProxy: http://${WORK_HOST}:8081/
noProxy: ${WORK_HOST}
proxy: http://${WORK_HOST}:8082/
noproxy: ${WORK_HOST}
registries:
  '@safe': https://safe.example.com/npm/
  '@work': https://${WORK_HOST}/scope/
namedRegistries:
  literal: 'https://registry.example.com/${/npm/'
  stable: https://registry.example.com/npm/
  work: https://${WORK_HOST}/npm/
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithHost>();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.pnpr_server, None);
    assert_eq!(config.registry, "https://registry.npmjs.org/");
    assert_eq!(config.proxy, pnpm_network::ProxyConfig::default());
    assert_eq!(
        config.registries_by_scope.get("@safe").map(String::as_str),
        Some("https://safe.example.com/npm/"),
    );
    assert_eq!(config.registries_by_scope.get("@work"), None);
    assert_eq!(
        config.registries_by_prefix.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("literal").map(String::as_str),
        Some("https://registry.example.com/${/npm/"),
    );
    assert_eq!(config.registries_by_prefix.get("work"), None);
}

#[test]
fn expands_env_vars_inside_non_registry_workspace_values() {
    struct EnvWithPaths;
    impl EnvVar for EnvWithPaths {
        fn var(name: &str) -> Option<String> {
            match name {
                "CACHE_DIR" => Some("cache-dir".to_owned()),
                "HOOK" => Some("hook.js".to_owned()),
                "SHELL" => Some("custom-shell".to_owned()),
                "STORE_DIR" => Some("store-dir".to_owned()),
                "USER_AGENT" => Some("pacquet-test/1.0".to_owned()),
                _ => None,
            }
        }
    }

    let yaml = r"
storeDir: ${STORE_DIR}
cacheDir: ${CACHE_DIR}
scriptShell: ${SHELL}
nodeOptions: --require=${HOOK}
userAgent: ${USER_AGENT}
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithPaths>();

    let base = Path::new("/workspace/root");
    let mut config = Config::new();
    settings.apply_to(&mut config, base);

    assert_eq!(config.store_dir, StoreDir::from(base.join("store-dir")));
    assert_eq!(config.cache_dir, base.join("cache-dir"));
    assert_eq!(config.script_shell.as_deref(), Some("custom-shell"));
    assert_eq!(config.node_options.as_deref(), Some("--require=hook.js"));
    assert_eq!(config.user_agent, "pacquet-test/1.0");
}

#[test]
fn keeps_non_ascii_text_in_workspace_values() {
    struct EnvWithPaths;
    impl EnvVar for EnvWithPaths {
        fn var(name: &str) -> Option<String> {
            match name {
                "CACHE_DIR" => Some("cache-dir".to_owned()),
                "STORE_DIR" => Some("store-dir".to_owned()),
                _ => None,
            }
        }
    }

    let yaml = r"
storeDir: ${STORE_DIR}/café
cacheDir: 日本語/${CACHE_DIR}
scriptShell: ./ünicode-shell
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithPaths>();

    let base = Path::new("/workspace/root");
    let mut config = Config::new();
    settings.resolve_script_shell(base);
    settings.apply_to(&mut config, base);

    assert_eq!(config.store_dir, StoreDir::from(base.join("store-dir/café")));
    assert_eq!(config.cache_dir, base.join("日本語/cache-dir"));
    let expected_script_shell =
        pnpm_fs::lexical_normalize(&base.join("./ünicode-shell")).to_string_lossy().into_owned();
    assert_eq!(config.script_shell.as_deref(), Some(expected_script_shell.as_str()));
}

/// `includeWorkspaceRoot` keeps the workspace root in a recursive
/// selection. Default `false`, so the yaml has to flip it on.
#[test]
fn parses_include_workspace_root_from_yaml_and_applies() {
    let yaml = "includeWorkspaceRoot: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.include_workspace_root, Some(true));

    let mut config = Config::new();
    assert!(!config.include_workspace_root, "the default is `false` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.include_workspace_root, "yaml override wins");
}

/// The two workspace-cycle knobs are independent keys — one silences the
/// report, the other promotes it to an error — so a file setting both is
/// applied to both fields.
#[test]
fn parses_the_workspace_cycle_settings_from_yaml_and_applies() {
    let yaml = "ignoreWorkspaceCycles: true\ndisallowWorkspaceCycles: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.ignore_workspace_cycles, Some(true));
    assert_eq!(settings.disallow_workspace_cycles, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_workspace_cycles, "the default is `false` to match pnpm");
    assert!(!config.disallow_workspace_cycles, "the default is `false` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_workspace_cycles, "yaml override wins");
    assert!(config.disallow_workspace_cycles, "yaml override wins");
}

/// `apply_to` records the workspace dir on `Config.workspace_dir`
/// (needed by `Config::resolved_patched_dependencies` so patch
/// file paths resolve against the same dir as upstream) and pushes
/// the raw map verbatim.
#[test]
fn apply_pushes_patched_dependencies_and_workspace_dir() {
    let yaml = r#"
patchedDependencies:
  "lodash@4.17.21": patches/lodash@4.17.21.patch
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let base = Path::new("/workspace/root");
    settings.apply_to(&mut config, base);

    assert_eq!(config.workspace_dir.as_deref(), Some(base));
    let map = config.patched_dependencies.expect("present");
    assert_eq!(map.get("lodash@4.17.21").map(String::as_str), Some("patches/lodash@4.17.21.patch"));
}

#[test]
fn patches_dir_reads_from_workspace_yaml() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("patchesDir: custom-patches\n").unwrap();
    assert_eq!(settings.patches_dir.as_deref(), Some("custom-patches"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace/root"));
    assert_eq!(config.patches_dir.as_deref(), Some("custom-patches"));
}

/// `testPattern` / `changedFilesIgnorePattern` cannot be set from the
/// global `config.yaml` — pnpm lists both in its excluded keys.
#[test]
fn test_pattern_and_changed_files_ignore_pattern_cleared_as_workspace_only_fields() {
    let yaml = r"
testPattern:
  - '*.spec.js'
changedFilesIgnorePattern:
  - '**/README.md'
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.test_pattern.is_none());
    assert!(settings.changed_files_ignore_pattern.is_none());
}

/// `versioning` is workspace-only: release plans must not be shaped by a
/// global `config.yaml`.
#[test]
fn versioning_cleared_as_workspace_only_field() {
    let yaml = r#"
versioning:
  lanes:
    "@example/cli": alpha
"#;
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.versioning.is_some());
    settings.clear_workspace_only_fields();
    assert!(settings.versioning.is_none());
}

/// `configDependencies` is workspace-only: it must not be honored from
/// the global `config.yaml`, matching pnpm's `isConfigFileKey` filter.
#[test]
fn config_dependencies_cleared_as_workspace_only_field() {
    let yaml = r#"
deployAllFiles: true
forceLegacyDeploy: true
sharedWorkspaceLockfile: false
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.deploy_all_files.is_none());
    assert!(settings.force_legacy_deploy.is_none());
    assert!(settings.shared_workspace_lockfile.is_none());
    assert!(settings.config_dependencies.is_none());
}

#[test]
fn cargo_settings_parse_apply_and_remain_workspace_only() {
    let yaml = r"
cargo:
  enabled: true
  indexUrl: https://registry.example.test/index/
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.cargo.as_ref().map(|cargo| cargo.enabled), Some(true));
    assert_eq!(
        settings.cargo.as_ref().map(|cargo| cargo.index_url.as_str()),
        Some("https://registry.example.test/index/"),
    );
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.cargo.enabled);
    assert_eq!(config.cargo.index_url, "https://registry.example.test/index/");
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.cargo.is_none());
}

#[test]
fn python_settings_parse_apply_and_remain_workspace_only() {
    let yaml = "python:\n  enabled: true\n  executable: python3.13\n  indexUrl: https://example.org/simple/\n  extras: [speed]\n  groups: [test]\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert!(config.python.enabled);
    assert_eq!(config.python.executable, "python3.13");
    assert_eq!(config.python.index_url, "https://example.org/simple/");
    assert_eq!(config.python.extras, ["speed"]);
    assert_eq!(config.python.groups, ["test"]);
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.python.is_none());
    assert!(serde_saphyr::from_str::<WorkspaceSettings>("python:\n  unknown: true\n").is_err());
}

/// The trust boundary follows the fields rather than the spelling, so the
/// canonical form must refuse everything the older one does — and the message
/// has to name the key the file actually wrote.
#[test]
fn rejects_workspace_controlled_trust_material_under_the_canonical_spelling() {
    for (trust_material, field) in [
        ("trustedKeys:\n      acme-2026: repository-controlled-key", "trustedKeys"),
        ("privateKey: repository-controlled-key", "privateKey"),
        ("publish: true", "publish"),
        ("keyId: acme-2026", "keyId"),
        ("builderId: ci/main/42", "builderId"),
        ("imageDigest: sha256:abc", "imageDigest"),
        ("architectureBaseline: x64", "architectureBaseline"),
        ("buildEnv:\n      CC: clang", "buildEnv"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!("sideEffectsCache:\n  remote:\n    org: acme\n    {trust_material}\n"),
        )
        .unwrap();

        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err().to_string();
        assert!(error.contains(&format!("sideEffectsCache.remote.{field}")), "{error}");
    }
}

/// A repository that could set `publish` would turn a key the machine holds for
/// its own builds into a signing oracle, so every field but the two that
/// declare eligibility is refused.
#[test]
fn rejects_workspace_controlled_shared_side_effects_trust_material() {
    for (trust_material, field) in [
        ("trustedKeys:\n    acme-2026: repository-controlled-key", "trustedKeys"),
        ("privateKey: repository-controlled-key", "privateKey"),
        ("publish: true", "publish"),
        ("keyId: acme-2026", "keyId"),
        ("builderId: ci/main/42", "builderId"),
        ("imageDigest: sha256:abc", "imageDigest"),
        ("architectureBaseline: x64", "architectureBaseline"),
        ("buildEnv:\n    CC: clang", "buildEnv"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!(
                "remoteSideEffectsCache:\n  organization: acme\n  packages:\n    - native-addon\n  {trust_material}\n",
            ),
        )
        .unwrap();

        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

/// The machine keeps the publication switch a repository may not touch.
#[test]
fn a_workspace_declaring_eligibility_keeps_the_machines_publication_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "remoteSideEffectsCache:\n  organization: acme\n  packages:\n    - native-addon\n",
    )
    .unwrap();
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  publish: true
  keyId: acme-2026
",
    )
    .unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/workspace"));
    WorkspaceSettings::load_at(dir.path()).unwrap().unwrap().apply_to(&mut config, dir.path());

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.publish, Some(true));
    assert_eq!(shared.key_id.as_deref(), Some("acme-2026"));
}

/// `linkWorkspacePackages` accepts `true | false | "deep"`.
#[test]
fn parses_link_workspace_packages_true_from_yaml() {
    let yaml = "linkWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::DirectOnly));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.link_workspace_packages, LinkWorkspacePackages::DirectOnly);
}

#[test]
fn parses_link_workspace_packages_false_from_yaml() {
    let yaml = "linkWorkspacePackages: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::Off));
}

#[test]
fn parses_link_workspace_packages_deep_from_yaml() {
    let yaml = "linkWorkspacePackages: deep\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::Deep));
}

#[test]
fn rejects_invalid_link_workspace_packages() {
    let yaml = "linkWorkspacePackages: shallow\n";
    serde_saphyr::from_str::<WorkspaceSettings>(yaml).expect_err("must reject");
}

/// `injectWorkspacePackages: true` propagates from yaml to
/// `Config.inject_workspace_packages`.
#[test]
fn parses_inject_workspace_packages_true_from_yaml() {
    let yaml = "injectWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, Some(true));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.inject_workspace_packages);
}

#[test]
fn parses_inject_workspace_packages_false_from_yaml() {
    let yaml = "injectWorkspacePackages: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, Some(false));

    let mut config = Config::new();
    config.inject_workspace_packages = true;
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.inject_workspace_packages);
}

#[test]
fn inject_workspace_packages_defaults_off_when_absent() {
    let yaml = "linkWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, None);

    let config = Config::new();
    assert!(!config.inject_workspace_packages);
}

/// A positive `workspaceConcurrency` is taken verbatim — same
/// resolution as `childConcurrency`.
#[test]
fn parses_positive_workspace_concurrency_from_yaml_and_applies() {
    let yaml = "workspaceConcurrency: 8\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.workspace_concurrency, Some(8));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.workspace_concurrency, 8);
}

/// A non-positive `workspaceConcurrency` is interpreted as
/// `max(1, parallelism - |value|)`. The exact result depends on the
/// host's reported parallelism, so bound-check it like the
/// `childConcurrency` sibling does.
#[test]
fn parses_negative_workspace_concurrency_from_yaml_and_resolves() {
    let yaml = "workspaceConcurrency: -1\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.workspace_concurrency, Some(-1));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let parallelism = crate::available_parallelism();
    assert!(config.workspace_concurrency >= 1, "must floor at 1");
    assert!(config.workspace_concurrency <= parallelism, "must not exceed available parallelism");
}

/// `workspaceConcurrency` and `childConcurrency` are independent
/// settings: setting one must not move the other off its default.
/// They are separate config keys.
#[test]
fn workspace_and_child_concurrency_are_independent() {
    let yaml = "workspaceConcurrency: 7\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, None);

    let mut config = Config::new();
    let child_default = config.child_concurrency;
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.workspace_concurrency, 7);
    assert_eq!(config.child_concurrency, child_default, "childConcurrency stays at its default");
}

/// `hoistPattern` and `publicHoistPattern` are tri-state via
/// [`super::super::deserialize_double_option`] — pacquet must distinguish
/// "key missing" (defaults stay) from "explicit null" (hoist
/// disabled) from "explicit list" (override). This test exercises
/// all three for both sides plus the `apply_to` plumbing.
#[test]
fn hoist_patterns_tri_state_round_trip() {
    let yaml = "registry: https://example.test\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, None);
    assert_eq!(settings.public_hoist_pattern, None);
    let mut config = Config::default();
    let defaults = (config.hoist_pattern.clone(), config.public_hoist_pattern.clone());
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!((config.hoist_pattern.clone(), config.public_hoist_pattern.clone()), defaults);

    let yaml = "hoistPattern: null\npublicHoistPattern: null\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, Some(None));
    assert_eq!(settings.public_hoist_pattern, Some(None));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.public_hoist_pattern, None);

    let yaml = "hoistPattern:\n  - 'foo*'\npublicHoistPattern: []\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, Some(Some(vec!["foo*".to_string()])));
    assert_eq!(settings.public_hoist_pattern, Some(Some(vec![])));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, Some(vec!["foo*".to_string()]));
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
}

/// `hoist: false` in `pnpm-workspace.yaml` nullifies
/// `Config.hoist_pattern` even when the user supplied an explicit
/// `hoistPattern` (or when the default `Some(["*"])` is in place):
/// `hoist === false ⇒ hoistPattern: undefined`. The install-time
/// `is_some() || is_some()` guard then short-circuits private
/// hoisting; `public_hoist_pattern` is intentionally untouched.
#[test]
fn hoist_false_disables_private_hoist_pattern() {
    let yaml = "hoist: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    let original_public = config.public_hoist_pattern.clone();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist, false);
    assert_eq!(config.hoist_pattern, None, "hoist:false must drop hoist_pattern");
    assert_eq!(
        config.public_hoist_pattern, original_public,
        "hoist:false must NOT touch public_hoist_pattern",
    );

    let yaml = "hoist: false\nhoistPattern:\n  - 'foo*'\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None, "hoist:false must override an explicit hoistPattern");
}

/// `hoistingLimits` deserializes as one of the `none` / `workspaces`
/// / `dependencies` modes; the install pipeline translates the mode
/// into the per-locator border map via
/// `pnpm_package_manager::get_hoisting_limits`. Yaml-empty /
/// missing keeps the `Config` field at its [`HoistingLimits::None`]
/// default.
#[test]
fn parses_hoisting_limits_from_yaml_and_applies() {
    let yaml = "hoistingLimits: dependencies\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoisting_limits, Some(HoistingLimits::Dependencies));

    let mut config = Config::new();
    assert_eq!(config.hoisting_limits, HoistingLimits::None, "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.hoisting_limits, HoistingLimits::Dependencies);
}

/// Both knobs absent → both `Config` fields stay at their empty
/// defaults. Pins the `apply_to` skip-on-None branch so future
/// edits don't accidentally overwrite with empty when the yaml
/// just doesn't mention these settings.
#[test]
fn omitting_hoisting_limits_and_external_dependencies_keeps_defaults() {
    let yaml = "";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.hoisting_limits.is_none());
    assert!(settings.external_dependencies.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.hoisting_limits, HoistingLimits::None);
    assert!(config.external_dependencies.is_empty());
}

/// `catalogMode` accepts the three upstream string values; an absent key
/// leaves the [`CatalogMode::Manual`] default in place.
#[test]
fn catalog_mode_yaml_values_round_trip() {
    for (yaml, expected) in [
        ("catalogMode: manual\n", CatalogMode::Manual),
        ("catalogMode: strict\n", CatalogMode::Strict),
        ("catalogMode: prefer\n", CatalogMode::Prefer),
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(settings.catalog_mode, Some(expected));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/irrelevant"));
        assert_eq!(config.catalog_mode, expected);
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str("").unwrap();
    assert!(settings.catalog_mode.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.catalog_mode,
        CatalogMode::Manual,
        "default stays manual when the key is absent",
    );
}

#[test]
fn resolves_relative_script_shell_against_workspace_root() {
    let base = Path::new("/workspace/root");
    for script_shell in ["./scripts/shell.sh", "../scripts/shell.sh", "scripts/shell.sh"] {
        let mut settings: WorkspaceSettings =
            serde_saphyr::from_str(&format!("scriptShell: {script_shell}")).unwrap();
        settings.resolve_script_shell(base);
        let mut config = Config::new();
        settings.apply_to(&mut config, base);
        let expected =
            pnpm_fs::lexical_normalize(&base.join(script_shell)).to_string_lossy().into_owned();
        assert_eq!(config.script_shell.as_deref(), Some(expected.as_str()));
    }
}

/// A drive-relative path (`C:tools\shell.cmd`) has a prefix but no root.
/// Node's `path.win32.join` keeps the workspace base in front of it.
#[cfg_attr(not(windows), ignore = "Windows path semantics")]
#[test]
fn resolves_drive_relative_script_shell_against_workspace_base() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str(r"scriptShell: 'C:tools\shell.cmd'").unwrap();
    settings.resolve_script_shell(Path::new(r"C:\workspace\root"));
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new(r"C:\workspace\root"));
    assert_eq!(config.script_shell.as_deref(), Some(r"C:\workspace\root\C:tools\shell.cmd"));
}

/// A declared `serverType` is workspace-only: it decides which tarball URLs
/// are omitted from the lockfile, so a user's global `config.yaml` must not
/// shape a lockfile their collaborators read back with a different layout. The
/// routes the same entry declares are a legitimate global preference and stay.
#[test]
fn registry_server_type_cleared_as_workspace_only_field() {
    let yaml = r"
registries:
  https://artifactory.example/artifactory/api/npm/npm-virtual/:
    serverType: artifactory
    scopes: ['@acme']
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    let entries = settings.registries.as_ref().expect("registries present");
    let RegistryEntry::Declaration(declaration) = entries.values().next().expect("one declaration")
    else {
        panic!("expected a declaration: {entries:?}")
    };
    assert_eq!(declaration.server_type, None);
    assert_eq!(declaration.scopes.as_deref(), Some(["@acme".to_owned()].as_slice()));
}

/// A `virtualStoreOnly` install keeps both hoist patterns empty whatever
/// hoist setting is unset beside it, and leaving that mode recomputes the
/// patterns from the settings still set rather than from the snapshot the
/// mode took.
#[test]
fn reset_setting_to_default_keeps_virtual_store_only_hoisting_empty() {
    let defaults = Config::default();
    let base_dir = Path::new("/tmp/project");
    let mut config = Config { virtual_store_only: true, hoist: false, ..Config::default() };
    config.apply_virtual_store_only_derivation();

    WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "hoist",
        base_dir,
    );
    assert!(config.hoist);
    assert_eq!(config.hoist_pattern, Some(Vec::new()));
    assert_eq!(config.public_hoist_pattern, Some(Vec::new()));

    config.explicit_settings.insert("hoistPattern".to_string(), serde_json::json!(["eslint-*"]));
    WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "virtualStoreOnly",
        base_dir,
    );
    assert!(!config.virtual_store_only);
    assert_eq!(config.hoist_pattern, Some(vec!["eslint-*".to_string()]));
    assert_eq!(config.public_hoist_pattern, defaults.public_hoist_pattern);
}
