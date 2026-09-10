use super::{
    Config, ConfigOverrides, OsString, PackageImportMethod, Path, PmOnFail, RuntimeOnFail,
    TrustPolicy, argv, assert_eq,
};

#[test]
fn extract_accepts_the_on_fail_settings_as_bare_flags() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--pm-on-fail=ignore",
        "--runtime-on-fail=warn",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.pm_on_fail, Some(PmOnFail::Ignore));
    assert_eq!(config.runtime_on_fail, Some(RuntimeOnFail::Warn));
}

#[test]
fn extract_leaves_other_bare_flags_for_clap() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--node-linker=hoisted"]));
    assert_eq!(remaining, argv(["pacquet", "install", "--node-linker=hoisted"]));
}

#[test]
fn extract_accepts_the_install_settings_as_bare_flags() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--package-import-method=hardlink",
        "--child-concurrency=3",
        "--strict-peer-dependencies",
        "--side-effects-cache",
        "--side-effects-cache-readonly",
        "--optimistic-repeat-install",
        "--trust-policy=no-downgrade",
        "--trust-policy-exclude=lodash",
        "--trust-policy-ignore-after=5",
        "--no-lockfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.package_import_method, PackageImportMethod::Hardlink);
    assert_eq!(config.child_concurrency, 3);
    assert!(config.strict_peer_dependencies);
    assert!(config.side_effects_cache);
    assert!(config.side_effects_cache_readonly);
    assert!(config.optimistic_repeat_install);
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude, Some(vec!["lodash".to_string()]));
    assert_eq!(config.trust_policy_ignore_after, Some(5));
    assert!(!config.lockfile);
}

/// `install` declares `--trust-lockfile` itself; every other command
/// takes the spelling from the table, so it lands on [`Config`] before
/// the command reads `config.trust_lockfile`.
#[test]
fn trust_lockfile_is_a_bare_flag_where_no_command_declares_it() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "remove", "foo", "--trust-lockfile"]));
    assert_eq!(remaining, argv(["pacquet", "remove", "foo"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.trust_lockfile);
    assert_eq!(config.explicit_settings.get("trustLockfile"), Some(&serde_json::Value::Bool(true)));

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "remove", "foo", "--no-trust-lockfile"]));
    assert_eq!(remaining, argv(["pacquet", "remove", "foo"]));
    let mut config = Config { trust_lockfile: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.trust_lockfile);

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.trust-lockfile=true", "update"]));
    assert_eq!(remaining, argv(["pacquet", "update"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.trust_lockfile);
}

/// Vercel runs every pnpm install as `pnpm install --unsafe-perm`
/// ([pnpm/pnpm#14346](https://github.com/pnpm/pnpm/issues/14346)).
#[test]
fn unsafe_perm_is_a_bare_flag_on_every_command() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--unsafe-perm"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config { unsafe_perm: false, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.unsafe_perm);
    assert_eq!(config.explicit_settings.get("unsafePerm"), Some(&serde_json::Value::Bool(true)));

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "rebuild", "--no-unsafe-perm"]));
    assert_eq!(remaining, argv(["pacquet", "rebuild"]));
    let mut config = Config { unsafe_perm: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.unsafe_perm);
    assert_eq!(config.explicit_settings.get("unsafePerm"), Some(&serde_json::Value::Bool(false)));

    for (flag, expected) in [
        ("--unsafe-perm", true),
        ("--unsafe-perm=true", true),
        ("--no-unsafe-perm", false),
        ("--unsafe-perm=false", false),
    ] {
        let (overrides, remaining) =
            ConfigOverrides::extract(argv(["pacquet", "remove", "foo", flag]));
        assert_eq!(remaining, argv(["pacquet", "remove", "foo"]), "{flag}");
        let mut config = Config { unsafe_perm: !expected, ..Config::default() };
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.unsafe_perm, expected, "{flag}");
    }
}

/// The boolean settings pnpm's `nopt` types make spellable on every
/// command that lists them, which clap rejected as unexpected arguments.
#[test]
fn the_boolean_settings_are_bare_flags_where_no_command_declares_them() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "foo",
        "--dangerously-allow-all-builds",
        "--engine-strict",
        "--frozen-store",
        "--lockfile-include-tarball-url",
        "--merge-git-branch-lockfiles",
        "--node-experimental-package-map",
        "--offline",
        "--prefer-frozen-lockfile",
        "--prefer-offline",
        "--no-shared-workspace-lockfile",
        "--no-verify-store-integrity",
        "--force-legacy-deploy",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.dangerously_allow_all_builds);
    assert!(config.engine_strict);
    assert!(config.frozen_store);
    assert!(config.lockfile_include_tarball_url);
    assert!(config.merge_git_branch_lockfiles);
    assert!(config.node_experimental_package_map);
    assert!(config.offline);
    assert!(config.prefer_frozen_lockfile);
    assert!(config.prefer_offline);
    assert!(!config.shared_workspace_lockfile);
    assert!(!config.verify_store_integrity);
    assert!(config.force_legacy_deploy);
    assert_eq!(config.explicit_settings.get("offline"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        config.explicit_settings.get("sharedWorkspaceLockfile"),
        Some(&serde_json::Value::Bool(false)),
    );

    // `audit` declares neither, unlike `install` and `add`.
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "audit",
        "--ignore-scripts",
        "--ignore-pnpmfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "audit"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.ignore_scripts);
    assert!(config.ignore_pnpmfile);
}

#[test]
fn a_boolean_setting_claims_the_next_token_only_when_it_spells_a_boolean() {
    for (tokens, expected) in [
        (vec!["--side-effects-cache", "install"], true),
        (vec!["--side-effects-cache", "false", "install"], false),
        (vec!["--side-effects-cache", "true", "install"], true),
    ] {
        let (overrides, remaining) =
            ConfigOverrides::extract(argv(["pacquet"]).into_iter().chain(argv(tokens)));
        assert_eq!(remaining, argv(["pacquet", "install"]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.side_effects_cache, expected);
    }
}

#[test]
fn a_setting_flag_does_not_swallow_the_command_it_precedes() {
    let (_, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--modules-dir",
        "custom_modules",
        "run",
        "build",
        "--hoist-pattern=eslint",
    ]));

    // `--hoist-pattern` is past the script name, so it is the script's.
    assert_eq!(remaining, argv(["pacquet", "run", "build", "--hoist-pattern=eslint"]));
}

/// A setting is stripped from argv before clap runs, so one spelled like
/// a *global* option would claim that option on every command line — a
/// command's own option is left for clap instead, and can collide.
#[test]
fn no_bare_setting_flag_shadows_a_global_option() {
    let grammar = crate::cli_args::grammar();
    let declared: Vec<&str> = grammar
        .get_arguments()
        .flat_map(|arg| {
            arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten())
        })
        .collect();
    let shadowed: Vec<&str> = super::super::tokens::BARE_SETTING_FLAGS
        .iter()
        .map(|&(setting, _)| setting)
        .filter(|setting| declared.contains(setting))
        .collect();
    assert_eq!(shadowed, Vec::<&str>::new());
}

/// A value the setting does not take must reach clap, which reports it:
/// dropping `--trust-policy=typo` would run the install under the default
/// `off` policy the user meant to replace.
#[test]
fn extract_leaves_invalid_setting_values_for_clap() {
    for tokens in [
        ["--trust-policy=typo"].as_slice(),
        &["--trust-policy", "typo"],
        &["--config.trust-policy=typo"],
        &["--package-import-method=symlink"],
        &["--child-concurrency=lots"],
        &["--child-concurrency", "lots"],
        &["--trust-policy-ignore-after=soon"],
        &["--link-workspace-packages=shallow"],
        &["--save-workspace-protocol=sometimes"],
        &["--offline=maybe"],
        &["--unsafe-perm=maybe"],
    ] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line.clone());
        assert_eq!(remaining, command_line, "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        let defaults = Config::default();
        assert_eq!(config.trust_policy, defaults.trust_policy, "{tokens:?}");
        assert_eq!(config.package_import_method, defaults.package_import_method, "{tokens:?}");
        assert_eq!(config.child_concurrency, defaults.child_concurrency, "{tokens:?}");
        assert_eq!(
            config.trust_policy_ignore_after, defaults.trust_policy_ignore_after,
            "{tokens:?}",
        );
    }
}

/// The boundary scan and the extraction have to agree on how much a bare
/// boolean setting claims, or the settings after an explicit `true` /
/// `false` are mistaken for a script's arguments and forwarded to clap.
#[test]
fn a_boolean_settings_explicit_value_does_not_move_the_command_boundary() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--strict-peer-dependencies",
        "false",
        "--config.registry=https://example.test/",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config { strict_peer_dependencies: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.strict_peer_dependencies);
    assert_eq!(config.registry, "https://example.test/");
}

/// `lockfile` is both a setting and `clean`'s own option, so the boundary
/// scan and the extraction have to agree on how much `--lockfile true`
/// claims even when the command is not `clean`.
#[test]
fn a_boolean_settings_value_is_claimed_even_when_a_command_declares_the_name() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--lockfile",
        "true",
        "--config.registry=https://example.test/",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config { lockfile: false, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.lockfile);
    assert_eq!(config.registry, "https://example.test/");
}

/// A `--` or another flag is never a free-form setting's value: claiming
/// one would drop the separator and point `modulesDir` at a directory
/// named `--`.
#[test]
fn a_setting_flag_never_claims_a_separator_or_another_flag() {
    for tokens in [
        ["--modules-dir", "--", "extra"].as_slice(),
        &["--modules-dir", "--"],
        &["--modules-dir", "--prod"],
        &["--modules-dir", "-C"],
    ] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line.clone());
        assert_eq!(remaining, command_line, "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.modules_dir, Config::default().modules_dir, "{tokens:?}");
    }
}

/// A parsed setting decides for itself: `childConcurrency` reads a
/// negative value as "every core but this many", so the flag claims it
/// even though it opens with `-`.
#[test]
fn a_numeric_setting_claims_a_negative_value() {
    for tokens in [["--child-concurrency", "-1"].as_slice(), &["--child-concurrency=-1"]] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line);
        assert_eq!(remaining, argv(["pacquet", "install"]), "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(
            config.child_concurrency,
            pnpm_config::resolve_child_concurrency(Some(-1)),
            "{tokens:?}",
        );
    }
}
