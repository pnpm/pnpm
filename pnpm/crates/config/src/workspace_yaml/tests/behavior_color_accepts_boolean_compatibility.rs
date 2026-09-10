use super::{
    AllowBuild, ColorMode, Config, Path, RegistryEntry, SideEffectsCacheSetting,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs,
};

#[test]
fn color_accepts_boolean_compatibility_values() {
    let always: WorkspaceSettings = serde_saphyr::from_str("color: true\n").unwrap();
    let never: WorkspaceSettings = serde_saphyr::from_str("color: false\n").unwrap();
    assert_eq!(always.color, Some(ColorMode::Always));
    assert_eq!(never.color, Some(ColorMode::Never));
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
fn load_at_buckets_the_problem_keys() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        concat!(
            "$schema: https://json.schemastore.org/pnpm-workspace.json\n",
            "configDir: /elsewhere\n",
            "minimumReleaseAg: 100\n",
            "store-dir: /some-store\n",
            "nodeLinker: hoisted\n",
            "globalShims:\n  node: true\n",
            "packages:\n  - apps/*\n",
        ),
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.refused, ["configDir"]);
    assert_eq!(settings.key_issues.unrecognized, ["minimumReleaseAg"]);
    assert_eq!(settings.key_issues.non_camel_case, ["store-dir"]);
}

#[test]
fn parses_and_applies_scope_from_yaml() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("scope: '@my-org'\n").unwrap();
    assert_eq!(settings.scope.as_deref(), Some("@my-org"));

    let mut config = Config::new();
    assert_eq!(config.scope, None);
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scope.as_deref(), Some("@my-org"));
}

#[test]
fn apply_scope_overrides_an_earlier_layer() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("scope: '@from-later-layer'\n").unwrap();
    let mut config = Config::new();
    config.scope = Some("@from-global-config".to_owned());
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scope.as_deref(), Some("@from-later-layer"));
}

/// `gitChecks: false` parses from `pnpm-workspace.yaml` and `apply_to`
/// pushes it onto [`Config::git_checks`], so a user can disable the
/// publish git checks via config exactly as pnpm's own hint instructs.
#[test]
fn parses_git_checks_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("gitChecks: false\n").unwrap();
    assert_eq!(settings.git_checks, Some(false));

    let mut config = Config::new();
    assert!(config.git_checks, "default is true");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.git_checks);
}

/// `namedRegistries` is the deprecated spelling of a registry's `prefix`. The
/// deserializer reads the camelCase key it still carries, and `apply_to`
/// writes the map onto [`Config::registries_by_prefix`] verbatim.
#[test]
fn parses_named_registries_from_yaml_and_applies() {
    let yaml = r"
namedRegistries:
  gh: https://npm.pkg.ghes.example.com/
  work: https://npm.work.example.com/
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let named = settings.named_registries.as_ref().expect("namedRegistries present");
    assert_eq!(named.get("gh").map(String::as_str), Some("https://npm.pkg.ghes.example.com/"));
    assert_eq!(named.get("work").map(String::as_str), Some("https://npm.work.example.com/"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("gh").map(String::as_str),
        Some("https://npm.pkg.ghes.example.com/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.work.example.com/"),
    );
}

#[test]
fn parses_registries_from_yaml_and_applies() {
    let yaml = r"
registries:
  default: https://default.example.com/npm
  '@private': https://private.example.com/npm
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let registries = settings.registries.as_ref().expect("registries present");
    assert_eq!(
        registries.get("default"),
        Some(&RegistryEntry::ScopeRoute("https://default.example.com/npm".to_owned())),
    );
    assert_eq!(
        registries.get("@private"),
        Some(&RegistryEntry::ScopeRoute("https://private.example.com/npm".to_owned())),
    );

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.registry, "https://default.example.com/npm/");
    assert_eq!(
        config.registries_by_scope.get("@private").map(String::as_str),
        Some("https://private.example.com/npm/"),
    );
}

/// `strictStorePkgContentCheck` decides whether a store row that holds
/// another package fails the install. Same camelCase rename +
/// `apply_to` wiring as `verifyStoreIntegrity`, and the same
/// default-true polarity.
#[test]
fn parses_strict_store_pkg_content_check_from_yaml_and_applies() {
    let yaml = "strictStorePkgContentCheck: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.strict_store_pkg_content_check, Some(false));

    let mut config = Config::new();
    assert!(config.strict_store_pkg_content_check, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.strict_store_pkg_content_check, "yaml override wins");
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
    assert_eq!(settings.side_effects_cache, Some(SideEffectsCacheSetting::Enabled(false)));

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

/// READ / WRITE gate helpers must combine the two knobs for the
/// canonical state combinations:
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
    assert_eq!(raw.get("esbuild").and_then(AllowBuild::decided), Some(true));
    assert_eq!(raw.get("foo@1.0.0").and_then(AllowBuild::decided), Some(true));
    assert_eq!(raw.get("bar").and_then(AllowBuild::decided), Some(false));

    let mut config = Config::new();
    assert!(config.allow_builds.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.allow_builds.get("esbuild").copied(), Some(true));
}

#[test]
fn parses_remote_side_effects_cache_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// A boolean says whether to read and write. It says nothing about the remote
/// tier, so one an earlier layer declared has to survive it — otherwise
/// `sideEffectsCache: false` in a project silently discards the org and
/// eligibility list the machine's global config set.
#[test]
fn a_later_shorthand_keeps_the_remote_tier() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: acme
    packages:
      - native-addon
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// Retaining a remote tier across a boolean must not change what the boolean
/// and `sideEffectsCacheReadonly` mean together: the read-only pair reads,
/// whether or not a remote tier was declared earlier.
#[test]
fn a_retained_remote_tier_does_not_change_the_read_only_pair() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: acme
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache: false
sideEffectsCacheReadonly: true
",
    )
    .unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read(), "the read-only pair still reads");
    assert!(!config.side_effects_cache_write());
    assert_eq!(config.remote_side_effects_cache.expect("shared cache config").org, "acme");
}

/// A file may carry both spellings of the field; `org` wins, and neither is a
/// parse error the way a serde alias would have made them.
#[test]
fn the_canonical_org_wins_over_the_alternative_spelling() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: canonical
    organization: alternative
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(config.remote_side_effects_cache.expect("shared cache config").org, "canonical");
}

/// Layers apply in order, so a shorthand in a later one has to beat an object
/// in an earlier one rather than being masked by what the object left behind.
#[test]
fn a_later_shorthand_overrides_an_earlier_object() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  read: true
  write: true
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// `organization` shipped in pacquet 12.0.0, so a file written for it keeps
/// working; `org` is what pnpr calls the same namespace.
#[test]
fn accepts_the_older_organization_spelling() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
}

#[test]
fn parses_the_canonical_side_effects_cache_declaration() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  read: true
  write: false
  remote:
    organization: acme
    packages:
      - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// Naming the remote tier says nothing about the local one, which was on by
/// default before this setting grew an object form.
#[test]
fn declaring_only_the_remote_tier_leaves_the_local_one_on() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());
}

#[test]
fn the_boolean_shorthand_still_reads_and_writes() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// The two spellings of the remote tier compose rather than replace, so a
/// repository may name the packages under one and the organization under the
/// other without either dropping the other's fields.
#[test]
fn the_canonical_declaration_wins_over_the_older_spellings() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCacheReadonly: true
remoteSideEffectsCache:
  packages:
    - from-the-old-key
sideEffectsCache:
  read: false
  write: true
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["from-the-old-key"]);
}

/// Precedence is by presence, not by validity: a malformed value under the name
/// matching the setting is not quietly replaced by a valid one under the older
/// name, because that would use a variable the reader did not reach for and
/// leave the broken one unreported.
#[test]
fn a_malformed_canonical_value_is_not_replaced_by_a_valid_older_one() {
    struct Both;
    impl crate::EnvVar for Both {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_SIDE_EFFECTS_CACHE_REMOTE_TRUSTED_KEYS" => Some("not json".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS" => {
                    Some(r#"{"acme-2026":"AA=="}"#.to_string())
                }
                _ => None,
            }
        }
    }

    let mut config = Config::new();
    let warnings = crate::tests::capture_warnings(|| {
        config.apply_remote_side_effects_cache_env::<Both>();
    });

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("not a string-valued JSON object"))
        .expect("a warning about the malformed variable");
    assert!(
        warning.contains("PNPM_SIDE_EFFECTS_CACHE_REMOTE_TRUSTED_KEYS"),
        "expected the variable that was selected, got {warning}",
    );
    assert!(
        config.remote_side_effects_cache.is_none_or(|shared| shared.trusted_keys.is_none()),
        "the older variable's value must not stand in for the malformed one",
    );
}

/// Each source contributes the half it owns, and the later one keeps what the
/// earlier one set.
#[test]
fn remote_side_effects_cache_sources_overlay_rather_than_replace() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  trustedKeys:
    acme-2026: AA==
  privateKey: BB==
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/workspace"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.trusted_keys.expect("trusted keys").get("acme-2026").unwrap(), "AA==");
    assert_eq!(shared.private_key.as_deref(), Some("BB=="));
}

/// pnpm scaffolds `allowBuilds` entries with a placeholder string for the
/// user to replace. The file pnpm wrote must stay loadable, and the
/// undecided package must stay under the default-deny policy rather than
/// becoming an explicit `false` (which `pnpm ignored-builds` would then
/// report as explicitly ignored).
#[test]
fn accepts_placeholder_strings_in_allow_builds() {
    let yaml = r"
allowBuilds:
  esbuild: set this to true or false
  sharp: true
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.allow_builds.clone().expect("field present");
    assert_eq!(
        raw.get("esbuild"),
        Some(&AllowBuild::Undecided("set this to true or false".to_string())),
    );

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.allow_builds.get("sharp").copied(), Some(true));
    assert_eq!(config.allow_builds.get("esbuild").copied(), None);
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

/// A positive `childConcurrency` is taken verbatim.
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
/// into [`pnpm_package_is_installable::check_platform`] via
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
