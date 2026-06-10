use super::{LoadWorkspaceYamlError, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings};
use crate::{
    CatalogMode, Config, HoistingLimits, LinkWorkspacePackages, NodeLinker, ResolutionMode,
    ScriptsPrependNodePath, TrustPolicy, api::EnvVar,
};
use pacquet_store_dir::StoreDir;
use pacquet_workspace_state::{ConfigDependency, ConfigDependencyDetail};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

#[test]
fn parses_common_settings_from_yaml() {
    let yaml = r"
storeDir: ../my-store
registry: https://reg.example
lockfile: false
autoInstallPeers: true
dedupePeers: true
preferWorkspacePackages: true
nodeLinker: hoisted
packages:
  - packages/*
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.store_dir.as_deref(), Some("../my-store"));
    assert_eq!(settings.registry.as_deref(), Some("https://reg.example"));
    assert_eq!(settings.lockfile, Some(false));
    assert_eq!(settings.auto_install_peers, Some(true));
    assert_eq!(settings.dedupe_peers, Some(true));
    assert_eq!(settings.prefer_workspace_packages, Some(true));
    assert!(matches!(settings.node_linker, Some(NodeLinker::Hoisted)));
}

#[test]
fn swallows_unknown_top_level_keys() {
    let yaml = r#"
catalog:
  react: ^18
onlyBuiltDependencies:
  - esbuild
packages:
  - "apps/*"
"#;
    // `pnpm-workspace.yaml` commonly contains top-level keys we do not
    // model in `WorkspaceSettings` (packages list, catalogs, build
    // allow-lists, ...). This guards against regressions that would make
    // serde reject those unknown keys during deserialization — i.e.
    // someone adding `deny_unknown_fields` to the struct.
    let _settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
}

#[test]
fn apply_overrides_npmrc_defaults() {
    let yaml = r"
storeDir: /absolute/store
lockfile: false
registry: https://reg.example
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    config.lockfile = true;
    let before_registry = config.registry.clone();

    settings.apply_to(&mut config, Path::new("/irrelevant-for-absolute-paths"));

    assert_eq!(config.store_dir, StoreDir::from(Path::new("/absolute/store").to_path_buf()));
    assert!(!config.lockfile);
    assert_eq!(config.registry, "https://reg.example/");
    assert_ne!(before_registry, config.registry);
}

#[test]
fn apply_resolves_relative_paths_against_base_dir() {
    let yaml = "storeDir: ../shared-store\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let base = Path::new("/workspace/root");

    settings.apply_to(&mut config, base);

    // Build the expected path via the same join machinery the code
    // under test uses so the component separator matches on every
    // platform (Windows uses `\` between joined components).
    assert_eq!(config.store_dir, StoreDir::from(base.join("../shared-store")));
}

/// pnpm reads `fetchRetries` / `fetchRetryFactor` /
/// `fetchRetryMintimeout` / `fetchRetryMaxtimeout` from
/// `pnpm-workspace.yaml` as camelCase keys (mirrors of the kebab-case
/// `.npmrc` form). Confirm both deserialization and `apply_to` push
/// the overrides onto the `Config`, since pacquet has to honour them
/// for parity with pnpm and for the install-time retry plumbing in
/// crates/tarball.
#[test]
fn parses_fetch_retry_settings_from_yaml_and_applies() {
    let yaml = r"
fetchRetries: 5
fetchRetryFactor: 3
fetchRetryMintimeout: 1000
fetchRetryMaxtimeout: 4000
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.fetch_retries, Some(5));
    assert_eq!(settings.fetch_retry_factor, Some(3));
    assert_eq!(settings.fetch_retry_mintimeout, Some(1000));
    assert_eq!(settings.fetch_retry_maxtimeout, Some(4000));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.fetch_retries, 5);
    assert_eq!(config.fetch_retry_factor, 3);
    assert_eq!(config.fetch_retry_mintimeout, 1000);
    assert_eq!(config.fetch_retry_maxtimeout, 4000);
}

/// `networkConcurrency` / `fetchTimeout` / `userAgent` parse from
/// `pnpm-workspace.yaml` as camelCase keys and `apply_to` pushes them
/// onto the `Config`, matching pnpm.
#[test]
fn parses_network_settings_from_yaml_and_applies() {
    let yaml = r"
networkConcurrency: 8
fetchTimeout: 120000
userAgent: my-agent/2.0
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.network_concurrency, Some(8));
    assert_eq!(settings.fetch_timeout, Some(120_000));
    assert_eq!(settings.user_agent.as_deref(), Some("my-agent/2.0"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.network_concurrency, 8);
    assert_eq!(config.fetch_timeout, 120_000);
    assert_eq!(config.user_agent, "my-agent/2.0");
}

/// `namedRegistries` is the per-alias registry-URL map from
/// `pnpm-workspace.yaml`. The deserializer reads the camelCase key
/// and `apply_to` writes the map onto [`Config::named_registries`]
/// verbatim. Mirrors upstream's
/// [`namedRegistries`](https://github.com/pnpm/pnpm/blob/b61e268d57/config/reader/src/Config.ts#L227)
/// schema.
#[test]
fn parses_named_registries_from_yaml_and_applies() {
    let yaml = r"
namedRegistries:
  gh: https://npm.pkg.ghes.example.com/
  work: https://npm.work.example.com/
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let named = settings.named_registries.as_ref().expect("named_registries present");
    assert_eq!(named.get("gh").map(String::as_str), Some("https://npm.pkg.ghes.example.com/"));
    assert_eq!(named.get("work").map(String::as_str), Some("https://npm.work.example.com/"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.named_registries.get("gh").map(String::as_str),
        Some("https://npm.pkg.ghes.example.com/"),
    );
    assert_eq!(
        config.named_registries.get("work").map(String::as_str),
        Some("https://npm.work.example.com/"),
    );
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
    assert_eq!(
        config.named_registries.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.named_registries.get("literal").map(String::as_str),
        Some("https://registry.example.com/${/npm/"),
    );
    assert_eq!(config.named_registries.get("work"), None);
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
fn trusted_settings_expand_env_vars_inside_request_destination_values() {
    struct EnvWithHost;
    impl EnvVar for EnvWithHost {
        fn var(name: &str) -> Option<String> {
            (name == "WORK_HOST").then(|| "internal.example.com".to_owned())
        }
    }

    let yaml = r"
pnprServer: https://${WORK_HOST}/pnpr/
registry: https://${WORK_HOST}/npm/
namedRegistries:
  stable: https://registry.example.com/npm/
  work: https://${WORK_HOST}/work/
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_trusted::<EnvWithHost>();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.pnpr_server.as_deref(), Some("https://internal.example.com/pnpr/"));
    assert_eq!(config.registry, "https://internal.example.com/npm/");
    assert_eq!(
        config.named_registries.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.named_registries.get("work").map(String::as_str),
        Some("https://internal.example.com/work/"),
    );
}

/// `verifyStoreIntegrity` is a camelCase key that serde's rename
/// has to pick up, and the `apply_to` wiring has to thread it onto
/// the `Config` field. Parse a yaml that flips the default-true
/// setting to false and assert both steps. Guards against silent
/// regressions in the key mapping or the apply step (a copy-paste
/// omission in `apply_to` would leave `config.verify_store_integrity`
/// at its default).
#[test]
fn parses_verify_store_integrity_from_yaml_and_applies() {
    let yaml = "verifyStoreIntegrity: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.verify_store_integrity, Some(false));

    let mut config = Config::new();
    assert!(config.verify_store_integrity, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.verify_store_integrity, "yaml override wins");
}

/// `sideEffectsCache` is the side-effects cache READ-path knob from
/// pnpm-workspace.yaml. Same shape as `verifyStoreIntegrity`:
/// camelCase rename + `apply_to` wiring. Parsing a yaml that flips
/// the default-true setting to false must end up at
/// `config.side_effects_cache == false`.
#[test]
fn parses_side_effects_cache_from_yaml_and_applies() {
    let yaml = "sideEffectsCache: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.side_effects_cache, Some(false));

    let mut config = Config::new();
    assert!(config.side_effects_cache, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.side_effects_cache, "yaml override wins");
}

/// `sideEffectsCacheReadonly` is pnpm's read-only flag for the
/// side-effects cache. Same camelCase + `apply_to` wiring as
/// `sideEffectsCache`. Default is `false`, so flipping it on via
/// yaml must end at `config.side_effects_cache_readonly == true`.
#[test]
fn parses_side_effects_cache_readonly_from_yaml_and_applies() {
    let yaml = "sideEffectsCacheReadonly: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.side_effects_cache_readonly, Some(true));

    let mut config = Config::new();
    assert!(!config.side_effects_cache_readonly, "the default is `false`");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.side_effects_cache_readonly, "yaml override wins");
}

/// READ / WRITE gate helpers must combine the two knobs the way
/// upstream's [`config/reader/src/index.ts`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/index.ts#L614-L615)
/// does for the canonical state combinations:
///
/// - default (`cache=true`, `readonly=false`)  → read=on, write=on
/// - cache off  (`cache=false`, `readonly=false`) → read=off, write=off
/// - readonly on (`cache=true`, `readonly=true`)  → read=on, write=off
/// - cache off + readonly on                      → read=on, write=off
#[test]
fn side_effects_cache_gates_truth_table() {
    let mut config = Config::new();
    assert!(config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());

    config.side_effects_cache = false;
    config.side_effects_cache_readonly = false;
    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());

    config.side_effects_cache = true;
    config.side_effects_cache_readonly = true;
    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());

    config.side_effects_cache = false;
    config.side_effects_cache_readonly = true;
    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// `patchedDependencies` in `pnpm-workspace.yaml` is a string→string
/// map where keys carry an optional `@version` suffix and values are
/// patch-file paths. pacquet captures it raw on `WorkspaceSettings`;
/// path resolution + hashing + grouping happen at install time via
/// `Config::resolved_patched_dependencies` (which delegates to
/// `pacquet_patching::resolve_and_group`). This test guards the
/// deserialization shape only — the camelCase rename, optionality,
/// and value-as-string-path.
#[test]
fn parses_patched_dependencies_from_yaml() {
    let yaml = r#"
patchedDependencies:
  "lodash@4.17.21": patches/lodash@4.17.21.patch
  "foo@^1.0.0": patches/foo.patch
  bar: patches/bar.patch
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.patched_dependencies.expect("field present");
    assert_eq!(map.get("lodash@4.17.21").map(String::as_str), Some("patches/lodash@4.17.21.patch"));
    assert_eq!(map.get("foo@^1.0.0").map(String::as_str), Some("patches/foo.patch"));
    assert_eq!(map.get("bar").map(String::as_str), Some("patches/bar.patch"));
}

#[test]
fn patched_dependencies_absent_yields_none() {
    let yaml = "storeDir: /s\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.patched_dependencies.is_none());
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

/// `configDependencies` is a map of package name → version-with-integrity
/// spec. pacquet records it into the workspace-state file so pnpm's
/// `checkDepsStatus` doesn't treat the install as stale on the next
/// `pnpm run` / `pnpm node`. Guards the camelCase rename, optionality,
/// and `apply_to` wiring.
#[test]
fn parses_config_dependencies_from_yaml_and_applies() {
    let yaml = r#"
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let expected = settings.config_dependencies.clone();
    assert_eq!(
        expected.as_ref().and_then(|m| m.get("@pnpm/pacquet")),
        Some(&ConfigDependency::VersionWithIntegrity("0.2.2-14".to_string())),
    );

    let mut config = Config::new();
    assert!(config.config_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.config_dependencies, expected);
}

/// pnpm's `configDependencies` value can also be the `{ tarball?, integrity }`
/// object form. It must parse (not error) and round-trip, otherwise an
/// upstream-supported manifest becomes a hard config-load failure.
#[test]
fn parses_object_form_config_dependencies() {
    let yaml = r#"
configDependencies:
  "@scope/dep":
    integrity: sha512-abc
    tarball: https://example.test/dep.tgz
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.config_dependencies.expect("field present");
    assert_eq!(
        map.get("@scope/dep"),
        Some(&ConfigDependency::Detailed(ConfigDependencyDetail {
            integrity: "sha512-abc".to_string(),
            tarball: Some("https://example.test/dep.tgz".to_string()),
        })),
    );
}

/// `configDependencies` is workspace-only: it must not be honored from
/// the global `config.yaml`, matching pnpm's `isConfigFileKey` filter.
#[test]
fn config_dependencies_cleared_as_workspace_only_field() {
    let yaml = r#"
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.config_dependencies.is_none());
}

/// `allowBuilds` is a map of `name[@version]` → bool. Same camelCase
/// rename + `apply_to` wiring as the other yaml-sourced settings.
/// pnpm 10+ moved this out of `package.json#pnpm` (matches
/// pnpm/pacquet#397 item 5).
#[test]
fn parses_allow_builds_from_yaml_and_applies() {
    let yaml = r#"
allowBuilds:
  esbuild: true
  "foo@1.0.0": true
  bar: false
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.allow_builds.clone().expect("field present");
    assert_eq!(raw.get("esbuild").copied(), Some(true));
    assert_eq!(raw.get("foo@1.0.0").copied(), Some(true));
    assert_eq!(raw.get("bar").copied(), Some(false));

    let mut config = Config::new();
    assert!(config.allow_builds.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.allow_builds.get("esbuild").copied(), Some(true));
}

/// `dangerouslyAllowAllBuilds` is a single boolean — default `false`
/// to match pnpm 11.
#[test]
fn parses_dangerously_allow_all_builds_from_yaml_and_applies() {
    let yaml = "dangerouslyAllowAllBuilds: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.dangerously_allow_all_builds, Some(true));

    let mut config = Config::new();
    assert!(!config.dangerously_allow_all_builds, "default is false");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.dangerously_allow_all_builds);
}

/// `scriptsPrependNodePath` is the tri-state from upstream
/// [`Config.scriptsPrependNodePath: boolean | 'warn-only'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts#L108).
/// `true` → Always, `false` → Never, `"warn-only"` → `WarnOnly`.
/// Pacquet's default is Never (matches upstream's
/// [`StrictBuildOptions.scriptsPrependNodePath: false`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/after-install/src/extendBuildOptions.ts#L78)).
#[test]
fn parses_scripts_prepend_node_path_true_from_yaml() {
    let yaml = "scriptsPrependNodePath: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Always));

    let mut config = Config::new();
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Never, "default Never");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Always);
}

#[test]
fn parses_scripts_prepend_node_path_false_from_yaml() {
    let yaml = "scriptsPrependNodePath: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Never));
}

#[test]
fn parses_scripts_prepend_node_path_warn_only_from_yaml() {
    let yaml = "scriptsPrependNodePath: warn-only\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::WarnOnly));
}

#[test]
fn rejects_invalid_scripts_prepend_node_path() {
    let yaml = "scriptsPrependNodePath: nonsense\n";
    serde_saphyr::from_str::<WorkspaceSettings>(yaml).expect_err("must reject");
}

/// `linkWorkspacePackages` accepts `true | false | "deep"`. Mirrors
/// upstream's [`Config.linkWorkspacePackages`](https://github.com/pnpm/pnpm/blob/5353fcbf01/config/reader/src/Config.ts#L189)
/// shape.
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
/// `Config.inject_workspace_packages`. Mirrors upstream's
/// [`Config.injectWorkspacePackages`](https://github.com/pnpm/pnpm/blob/39101f5e37/config/reader/src/Config.ts#L190).
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

/// `unsafePerm: false` from yaml propagates to `Config.unsafe_perm`
/// on POSIX. Mirrors upstream's [`Config.unsafePerm: boolean`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts).
/// The starting `Config::new()` value depends on the runtime uid
/// (see [`default_unsafe_perm`]) — `true` for non-root, `false`
/// for root. Either way, `apply_to` with `Some(false)` ends in
/// `false`.
#[test]
fn parses_unsafe_perm_from_yaml_and_applies() {
    // POSIX-only: the Windows force-override below would mask this
    // test's behavior. See [`WorkspaceSettings::apply_to`].
    if cfg!(windows) {
        return;
    }
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.unsafe_perm, Some(false));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.unsafe_perm, "yaml override wins on POSIX");
}

/// On Windows, `apply_to` ignores the yaml value and forces
/// `unsafe_perm = true`. Mirrors upstream's
/// [`process.platform === 'win32'` override](https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L204-L220)
/// — running lifecycle scripts under a uid/gid drop is POSIX-only.
#[cfg(windows)]
#[test]
fn unsafe_perm_force_true_on_windows() {
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("C:/irrelevant"));
    assert!(config.unsafe_perm, "Windows forces unsafe_perm true regardless of yaml");
}

/// A positive `childConcurrency` is taken verbatim — mirrors
/// upstream's [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34).
#[test]
fn parses_positive_child_concurrency_from_yaml_and_applies() {
    let yaml = "childConcurrency: 8\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, Some(8));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.child_concurrency, 8);
}

/// A non-positive `childConcurrency` is interpreted as
/// `max(1, parallelism - |value|)`. The exact result depends on
/// the host's reported parallelism, so we just bound-check it:
/// negative offsets must produce at least 1 and at most
/// `parallelism()`.
#[test]
fn parses_negative_child_concurrency_from_yaml_and_resolves() {
    let yaml = "childConcurrency: -1\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, Some(-1));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let parallelism = crate::available_parallelism();
    assert!(config.child_concurrency >= 1, "must floor at 1");
    assert!(config.child_concurrency <= parallelism, "must not exceed available parallelism");
}

/// A positive `workspaceConcurrency` is taken verbatim — same
/// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34)
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
/// Mirrors upstream, where they are separate config keys
/// ([`config/reader/src/index.ts:208`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L208)
/// vs the `childConcurrency` build path).
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

#[test]
fn apply_leaves_unset_fields_alone() {
    let yaml = "storeDir: /s\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let before =
        (config.hoist, config.lockfile, config.registry.clone(), config.auto_install_peers);

    settings.apply_to(&mut config, Path::new("/anywhere"));

    assert_eq!(
        (config.hoist, config.lockfile, config.registry.clone(), config.auto_install_peers),
        before,
    );
}

#[test]
fn find_walks_up_to_parent_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "storeDir: /s\n").unwrap();

    let (found, settings) = WorkspaceSettings::find_and_load(&nested).unwrap().unwrap();
    assert_eq!(found, tmp.path().join("pnpm-workspace.yaml"));
    assert_eq!(settings.store_dir.as_deref(), Some("/s"));
}

/// Pnpm's `readManifestRaw` only treats `ENOENT` as "no manifest" and
/// propagates every other failure. A directory entry named
/// `pnpm-workspace.yaml` is not a missing file, so `find_and_load`
/// must surface it as `ReadFile` rather than silently walking up.
#[test]
fn find_propagates_when_manifest_path_is_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    tmp.path().join(WORKSPACE_MANIFEST_FILENAME).pipe(fs::create_dir).unwrap();

    let err = tmp
        .path()
        .pipe_as_ref(WorkspaceSettings::find_and_load)
        .expect_err("a directory at the manifest path is not a missing file");
    assert!(
        matches!(err, LoadWorkspaceYamlError::ReadFile { .. }),
        "expected ReadFile, got {err:?}",
    );

    drop(tmp); // clean up
}

/// A `pnpm-workspace.yaml` whose contents do not parse as YAML must
/// surface as `ParseYaml` (not `ReadFile`, not silently dropped),
/// matching pnpm's `readManifestRaw` behaviour where parse failures
/// abort the install rather than fall through to defaults.
#[test]
fn find_propagates_parse_yaml_error_on_malformed_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = tmp.path().join(WORKSPACE_MANIFEST_FILENAME);
    // Unmatched bracket; serde-saphyr rejects.
    fs::write(&manifest, "storeDir: [unterminated\n").unwrap();

    let err = WorkspaceSettings::find_and_load(tmp.path())
        .expect_err("malformed yaml must surface as ParseYaml");
    let LoadWorkspaceYamlError::ParseYaml { path, .. } = err else {
        panic!("expected ParseYaml, got {err:?}");
    };
    assert_eq!(path, manifest);

    drop(tmp); // clean up
}

#[test]
fn find_returns_none_when_no_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(WorkspaceSettings::find_and_load(tmp.path()).unwrap().is_none());
}

#[test]
fn apply_replaces_git_shallow_hosts_defaults() {
    // pnpm replaces the built-in default array wholesale rather than
    // merging it, so we mirror that. See `default_git_shallow_hosts`.
    let yaml = r"
gitShallowHosts:
  - corp-git.example.com
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();

    // Sanity-check the default before applying — `github.com` is the
    // first entry in pnpm's list, and replacement (not merging) is the
    // bit we want to verify.
    assert!(config.git_shallow_hosts.iter().any(|host| host == "github.com"));

    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert_eq!(config.git_shallow_hosts, vec!["corp-git.example.com".to_string()]);
}

/// `supportedArchitectures` from `pnpm-workspace.yaml`. Optional
/// `os` / `cpu` / `libc` lists; absent fields stay `None`. Threaded
/// into [`pacquet_package_is_installable::check_platform`] via
/// [`Config::supported_architectures`] at install time.
#[test]
fn parses_supported_architectures_from_yaml_and_applies() {
    let yaml = r"
supportedArchitectures:
  os: [darwin, linux]
  cpu: [arm64, x64]
  libc: [glibc]
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.supported_architectures.clone().expect("field present");
    assert_eq!(raw.os.as_deref(), Some(&["darwin".to_string(), "linux".to_string()][..]));
    assert_eq!(raw.cpu.as_deref(), Some(&["arm64".to_string(), "x64".to_string()][..]));
    assert_eq!(raw.libc.as_deref(), Some(&["glibc".to_string()][..]));

    let mut config = Config::new();
    assert!(config.supported_architectures.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.supported_architectures.expect("set after apply_to");
    assert_eq!(applied.os.as_deref(), Some(&["darwin".to_string(), "linux".to_string()][..]));
    assert_eq!(applied.cpu.as_deref(), Some(&["arm64".to_string(), "x64".to_string()][..]));
    assert_eq!(applied.libc.as_deref(), Some(&["glibc".to_string()][..]));
}

/// Absent `supportedArchitectures` leaves the config field at
/// `None`. Same shape as upstream: yaml-side absence translates to
/// `targetConfig.supportedArchitectures` staying `undefined` so the
/// per-axis check falls back to the host triple.
#[test]
fn omitting_supported_architectures_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.supported_architectures.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.supported_architectures.is_none());
}

/// Partial `supportedArchitectures` (only one axis set) round-trips
/// with the other axes as `None`. Matches upstream where each axis
/// is independently overridable.
#[test]
fn partial_supported_architectures_only_sets_listed_axes() {
    let yaml = r"
supportedArchitectures:
  os: [darwin]
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.supported_architectures.expect("field present");
    assert_eq!(raw.os.as_deref(), Some(&["darwin".to_string()][..]));
    assert!(raw.cpu.is_none());
    assert!(raw.libc.is_none());
}

/// `hoistPattern` and `publicHoistPattern` are tri-state via
/// [`super::deserialize_double_option`] — pacquet must distinguish
/// "key missing" (defaults stay) from "explicit null" (hoist
/// disabled) from "explicit list" (override). This test exercises
/// all three for both sides plus the `apply_to` plumbing.
#[test]
fn hoist_patterns_tri_state_round_trip() {
    // Case 1: keys absent → defaults preserved.
    let yaml = "registry: https://example.test\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, None);
    assert_eq!(settings.public_hoist_pattern, None);
    let mut config = Config::default();
    let defaults = (config.hoist_pattern.clone(), config.public_hoist_pattern.clone());
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!((config.hoist_pattern.clone(), config.public_hoist_pattern.clone()), defaults);

    // Case 2: explicit null → `Config.* = None`. Verifies the
    // upstream `!= null` semantics — null disables that side, and
    // the install-time `is_some() || is_some()` guard short-circuits
    // when both sides are None.
    let yaml = "hoistPattern: null\npublicHoistPattern: null\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, Some(None));
    assert_eq!(settings.public_hoist_pattern, Some(None));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.public_hoist_pattern, None);

    // Case 3: explicit list → wraps the inner Vec in `Some`.
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
/// `hoistPattern` (or when the default `Some(["*"])` is in place).
/// Mirrors upstream's
/// [`projectConfig.ts:72-75`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/projectConfig.ts#L72-L75)
/// `result.hoist === false ⇒ hoistPattern: undefined`. The install-
/// time `is_some() || is_some()` guard then short-circuits private
/// hoisting; `public_hoist_pattern` is intentionally untouched
/// (upstream doesn't nullify it either).
#[test]
fn hoist_false_disables_private_hoist_pattern() {
    // `hoist: false` alone — the default `hoist_pattern` should drop.
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

    // `hoist: false` wins over an explicit `hoistPattern` — yaml
    // sets a pattern, but `hoist: false` then nullifies it.
    let yaml = "hoist: false\nhoistPattern:\n  - 'foo*'\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None, "hoist:false must override an explicit hoistPattern");
}

/// `ignoredOptionalDependencies` parses from yaml as a list of
/// strings and applies onto `Config::ignored_optional_dependencies`
/// verbatim — order preserved, no sorting at apply time (the
/// freshness check sorts before comparison, but `Config` holds the
/// user-supplied order). Mirrors upstream's
/// [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts).
#[test]
fn parses_ignored_optional_dependencies_from_yaml_and_applies() {
    let yaml = r"
ignoredOptionalDependencies:
  - 'foo'
  - '@scope/bar'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(
        settings.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );

    let mut config = Config::new();
    assert!(config.ignored_optional_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );
}

/// Absent `ignoredOptionalDependencies` leaves the config field at
/// `None` (same convention as `supportedArchitectures`).
#[test]
fn omitting_ignored_optional_dependencies_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.ignored_optional_dependencies.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignored_optional_dependencies.is_none());
}

/// `overrides` parses as an ordered string→string map and applies
/// onto `Config::overrides`. Order is preserved because the field is
/// an `IndexMap` — pnpm's lockfile-drift comparison is
/// order-insensitive, but the read-package hook iterates the map and
/// downstream diagnostics reference the keys in user-supplied order.
#[test]
fn parses_overrides_from_yaml_and_applies() {
    let yaml = r"
overrides:
  foo: '1.2.3'
  '@scope/bar': '^2.0.0'
  'baz>qux': '-'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let overrides = settings.overrides.as_ref().expect("overrides parsed");
    let entries: Vec<_> =
        overrides.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    assert_eq!(entries, vec![("foo", "1.2.3"), ("@scope/bar", "^2.0.0"), ("baz>qux", "-")]);

    let mut config = Config::new();
    assert!(config.overrides.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.overrides.expect("overrides applied");
    assert_eq!(applied.get("foo").map(String::as_str), Some("1.2.3"));
    assert_eq!(applied.get("@scope/bar").map(String::as_str), Some("^2.0.0"));
    assert_eq!(applied.get("baz>qux").map(String::as_str), Some("-"));
}

/// An empty `overrides:` map collapses to `None` on `Config`, matching
/// upstream's `delete settings.overrides` short-circuit in
/// `getOptionsFromPnpmSettings`. Without this collapse, an empty
/// `overrides: {}` would diverge from "no key set" at the lockfile-
/// drift comparison.
#[test]
fn empty_overrides_map_collapses_to_none() {
    let yaml = "overrides: {}\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.overrides.as_ref().is_some_and(indexmap::IndexMap::is_empty));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none(), "empty map collapses to None");
}

/// An explicit `overrides: {}` from a later layer (env overlay,
/// later `apply_to` call) clears a non-empty value set by an earlier
/// layer. Without the empty-clears-prior semantic, an env override
/// like `PNPM_CONFIG_OVERRIDES={}` would be a silent no-op against a
/// non-empty workspace yaml.
#[test]
fn empty_overrides_clears_prior_non_empty_assignment() {
    let mut config = Config::new();
    let yaml_with_overrides = "overrides:\n  foo: '1.2.3'\n";
    let earlier: WorkspaceSettings = serde_saphyr::from_str(yaml_with_overrides).unwrap();
    earlier.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_some(), "non-empty overrides applied");

    let later: WorkspaceSettings = serde_saphyr::from_str("overrides: {}\n").unwrap();
    later.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none(), "explicit empty must clear earlier non-empty");
}

/// Absent `overrides` leaves the config field at `None`.
#[test]
fn omitting_overrides_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.overrides.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none());
}

/// `packageExtensions` parses as an ordered `selector → entry` map
/// and applies onto [`Config::package_extensions`]. The entry uses
/// camelCase field names so inner sections like
/// `optionalDependencies` and `peerDependenciesMeta` round-trip
/// through the deserializer.
#[test]
fn parses_package_extensions_from_yaml_and_applies() {
    let yaml = r#"
packageExtensions:
  is-positive:
    dependencies:
      "@pnpm.e2e/bar": 100.1.0
  "@scope/foo@^2":
    peerDependencies:
      react: ">=16"
    peerDependenciesMeta:
      react:
        optional: true
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let extensions = settings.package_extensions.as_ref().expect("packageExtensions parsed");
    let is_positive = extensions.get("is-positive").expect("is-positive entry");
    assert_eq!(
        is_positive
            .dependencies
            .as_ref()
            .and_then(|map| map.get("@pnpm.e2e/bar"))
            .map(String::as_str),
        Some("100.1.0"),
    );
    let scoped = extensions.get("@scope/foo@^2").expect("scoped entry");
    assert_eq!(
        scoped.peer_dependencies.as_ref().and_then(|map| map.get("react")).map(String::as_str),
        Some(">=16"),
    );
    let meta = scoped
        .peer_dependencies_meta
        .as_ref()
        .and_then(|map| map.get("react"))
        .expect("react peerDependenciesMeta entry");
    assert_eq!(meta.optional, Some(true));

    let mut config = Config::new();
    assert!(config.package_extensions.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.package_extensions.expect("package_extensions applied");
    assert_eq!(applied.len(), 2);
}

/// An empty `packageExtensions:` map collapses to `None` on
/// `Config`, mirroring the `overrides` behavior. Without this
/// collapse, an empty `{}` would diverge from "no key set" at the
/// workspace-state drift comparison.
#[test]
fn empty_package_extensions_map_collapses_to_none() {
    let yaml = "packageExtensions: {}\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.package_extensions.as_ref().is_some_and(indexmap::IndexMap::is_empty));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.package_extensions.is_none(), "empty map collapses to None");
}

/// `hoistingLimits` deserializes as one of the `none` / `workspaces`
/// / `dependencies` modes. Mirrors upstream's
/// [`HoistingLimits`](https://github.com/pnpm/pnpm/blob/89812a9353/installing/linking/real-hoist/src/index.ts)
/// shape; the install pipeline translates the mode into the
/// per-locator border map via `pacquet_package_manager::get_hoisting_limits`.
/// Yaml-empty / missing keeps the `Config` field at its
/// [`HoistingLimits::None`] default.
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

/// `externalDependencies` deserializes as a flat list of names.
/// Yaml-empty / missing keeps the `Config` field at its
/// `BTreeSet::default()` empty value.
#[test]
fn parses_external_dependencies_from_yaml_and_applies() {
    let yaml = r"
externalDependencies:
  - bit-bin
  - some-other-external
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.external_dependencies.clone().expect("field present");
    assert!(raw.contains("bit-bin") && raw.contains("some-other-external"));

    let mut config = Config::new();
    assert!(config.external_dependencies.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.external_dependencies.contains("bit-bin"));
    assert!(config.external_dependencies.contains("some-other-external"));
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

/// Lockfile-verification policy keys all live in `pnpm-workspace.yaml`
/// alongside the rest of the install settings. This test asserts the
/// camelCase rename + `apply_to` wiring for every new field
/// introduced by the gate: `cacheDir` (path-resolved against the
/// workspace dir), `minimumReleaseAge` / `…Exclude` / `…Strict` /
/// `…IgnoreMissingTime`, and `trustPolicy` / `…Exclude` /
/// `…IgnoreAfter`. Mirrors the upstream key list at
/// <https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L264-L272>.
#[test]
fn parses_supply_chain_policy_settings_from_yaml_and_applies() {
    let yaml = r#"
cacheDir: ./.pacquet-cache
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - lodash
  - "is-*"
minimumReleaseAgeIgnoreMissingTime: true
minimumReleaseAgeStrict: true
trustLockfile: true
trustPolicy: no-downgrade
trustPolicyExclude:
  - "@scope/legacy"
trustPolicyIgnoreAfter: 525600
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.cache_dir.as_deref(), Some("./.pacquet-cache"));
    assert_eq!(settings.minimum_release_age, Some(1440));
    assert_eq!(
        settings.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert_eq!(settings.minimum_release_age_ignore_missing_time, Some(true));
    assert_eq!(settings.minimum_release_age_strict, Some(true));
    assert_eq!(settings.trust_lockfile, Some(true));
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));
    assert_eq!(settings.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(settings.trust_policy_ignore_after, Some(525_600));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/proj"));
    assert_eq!(config.cache_dir, Path::new("/proj/.pacquet-cache"));
    assert_eq!(config.minimum_release_age, Some(1440));
    assert_eq!(
        config.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert!(config.minimum_release_age_ignore_missing_time);
    assert_eq!(config.minimum_release_age_strict, Some(true));
    assert!(config.resolved_minimum_release_age_strict());
    assert!(config.trust_lockfile);
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(config.trust_policy_ignore_after, Some(525_600));
}

/// `trustPolicy` accepts the two upstream string values; an absent
/// key leaves the [`TrustPolicy::Off`] default in place.
#[test]
fn trust_policy_yaml_values_round_trip() {
    let yaml = "trustPolicy: off\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::Off));

    let yaml = "trustPolicy: no-downgrade\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));

    let yaml = "";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.trust_policy.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.trust_policy, TrustPolicy::Off, "default stays off when key is absent");
}

/// `resolutionMode` accepts the three upstream string values; an
/// absent key leaves the [`ResolutionMode::Highest`] default in place.
#[test]
fn resolution_mode_yaml_values_round_trip() {
    for (yaml, expected) in [
        ("resolutionMode: highest\n", ResolutionMode::Highest),
        ("resolutionMode: time-based\n", ResolutionMode::TimeBased),
        ("resolutionMode: lowest-direct\n", ResolutionMode::LowestDirect),
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(settings.resolution_mode, Some(expected));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/irrelevant"));
        assert_eq!(config.resolution_mode, expected);
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str("").unwrap();
    assert!(settings.resolution_mode.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.resolution_mode,
        ResolutionMode::Highest,
        "default stays highest when the key is absent",
    );
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

/// `registrySupportsTimeField` is a camelCase boolean; default `false`.
#[test]
fn parses_registry_supports_time_field_from_yaml_and_applies() {
    let yaml = "registrySupportsTimeField: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.registry_supports_time_field, Some(true));

    let mut config = Config::new();
    assert!(!config.registry_supports_time_field, "the default is `false`");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.registry_supports_time_field, "yaml override wins");
}

/// `allowedDeprecatedVersions` is a `name → semver-range` map parsed
/// from camelCase yaml and applied verbatim onto `Config`.
#[test]
fn parses_allowed_deprecated_versions_from_yaml_and_applies() {
    let yaml = r#"
allowedDeprecatedVersions:
  request: "^2.88.0"
  lodash: "<5.0.0"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.allowed_deprecated_versions.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.allowed_deprecated_versions.get("request").map(String::as_str),
        Some("^2.88.0"),
    );
    assert_eq!(
        config.allowed_deprecated_versions.get("lodash").map(String::as_str),
        Some("<5.0.0"),
    );
}

/// `updateConfig.ignoreDependencies` parses from the nested camelCase
/// shape and lands on `Config.update_config`.
#[test]
fn parses_update_config_ignore_dependencies_from_yaml_and_applies() {
    let yaml = r#"
updateConfig:
  ignoreDependencies:
    - "@pnpm.e2e/foo"
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.update_config.ignore_dependencies.is_none(), "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string(), "@pnpm.e2e/bar".to_string()][..]),
    );
}

/// `peerDependencyRules` parses its three sub-fields from camelCase
/// yaml and lands on `Config.peer_dependency_rules`.
#[test]
fn parses_peer_dependency_rules_from_yaml_and_applies() {
    let yaml = r#"
peerDependencyRules:
  ignoreMissing:
    - ajv
  allowAny:
    - react
  allowedVersions:
    bbb: "2"
    "xxx>@foo/bar": "2"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert_eq!(
        config.peer_dependency_rules,
        crate::PeerDependencyRules::default(),
        "default is empty",
    );
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let rules = &config.peer_dependency_rules;
    assert_eq!(rules.ignore_missing.as_deref(), Some(&["ajv".to_string()][..]));
    assert_eq!(rules.allow_any.as_deref(), Some(&["react".to_string()][..]));
    let allowed = rules.allowed_versions.as_ref().expect("allowedVersions set");
    assert_eq!(allowed.get("bbb").map(String::as_str), Some("2"));
    assert_eq!(allowed.get("xxx>@foo/bar").map(String::as_str), Some("2"));
}

/// `scriptShell` and `nodeOptions` parse from `pnpm-workspace.yaml` as
/// camelCase keys and `apply_to` writes them to the corresponding
/// `Config` fields. A present string deserializes to `Some(Some(_))`.
#[test]
fn parses_script_shell_and_node_options_from_yaml_and_applies() {
    let yaml = r"
scriptShell: /usr/bin/bash
nodeOptions: --max-old-space-size=4096
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.script_shell, Some(Some("/usr/bin/bash".to_string())));
    assert_eq!(settings.node_options, Some(Some("--max-old-space-size=4096".to_string())));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/usr/bin/bash"));
    assert_eq!(config.node_options.as_deref(), Some("--max-old-space-size=4096"));
}

/// The tri-state distinguishes "absent" from "explicit null", matching
/// pnpm: an explicit `scriptShell: null` / `nodeOptions: null` clears a
/// value inherited from global `config.yaml`, while an absent key leaves
/// the inherited value untouched.
#[test]
fn script_shell_and_node_options_null_clears_inherited_value() {
    // An absent key parses to `None` and `apply_to` keeps the inherited value.
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.script_shell, None);
    assert_eq!(absent.node_options, None);

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/inherited/sh"), "absent must inherit");
    assert_eq!(config.node_options.as_deref(), Some("--inherited"), "absent must inherit");

    // An explicit `null` parses to `Some(None)` and `apply_to` clears it.
    let cleared: WorkspaceSettings =
        serde_saphyr::from_str("scriptShell: null\nnodeOptions: null").unwrap();
    assert_eq!(cleared.script_shell, Some(None));
    assert_eq!(cleared.node_options, Some(None));

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    cleared.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell, None, "explicit null must clear the inherited shell");
    assert_eq!(config.node_options, None, "explicit null must clear inherited NODE_OPTIONS");
}
