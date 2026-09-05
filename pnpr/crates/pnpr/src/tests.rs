use super::{Args, RegistryError, redacted_report};
use clap::{CommandFactory as _, Parser as _};
use pnpm_testing_utils::env_guard::EnvGuard;
use std::{ffi::OsStr, net::SocketAddr};

const ENV_VARS: [&str; 11] = [
    "PNPR_CONFIG",
    "PNPR_LISTEN",
    "PNPR_STORAGE",
    "PNPR_CACHE",
    "PNPR_PUBLIC_URL",
    "PNPR_PACKUMENT_TTL_SECS",
    "PNPR_OSV",
    "PNPR_OSV_DB",
    "PNPR_DISABLE_REGISTRY",
    "PNPR_DISABLE_RESOLVER",
    "PNPR_DISABLE_ARTIFACTS",
];

/// Hold the env lock with every `PNPR_*` variable cleared, so a parse sees
/// only what the test sets and never what the developer's shell exports.
fn scrubbed_env() -> EnvGuard {
    let env = EnvGuard::snapshot(ENV_VARS);
    for var in ENV_VARS {
        // SAFETY: `env` holds the process-wide env-mutation lock and restores
        // every variable in `ENV_VARS` on drop.
        unsafe { std::env::remove_var(var) };
    }
    env
}

#[test]
fn every_flag_is_bound_to_the_env_var_named_after_it() {
    let mut bound = Vec::new();
    for arg in Args::command().get_arguments() {
        let long = arg.get_long().expect("every pnpr option has a long flag");
        let expected = format!("PNPR_{}", long.to_uppercase().replace('-', "_"));
        assert_eq!(arg.get_env(), Some(OsStr::new(&expected)), "--{long}");
        bound.push(expected);
    }
    assert_eq!(bound, ENV_VARS);
}

#[test]
fn disable_artifacts_sets_the_config_override() {
    let _env = scrubbed_env();
    let args = Args::try_parse_from(["pnpr", "--disable-artifacts"]).unwrap();

    let overrides = args.feature_overrides();

    assert!(overrides.disable_artifacts);
    assert!(!overrides.disable_registry);
    assert!(!overrides.disable_resolver);
}

#[test]
fn env_vars_stand_in_for_omitted_flags() {
    let env = scrubbed_env();
    env.set("PNPR_CONFIG", "/etc/pnpr/config.yaml");
    env.set("PNPR_LISTEN", "0.0.0.0:4873");
    env.set("PNPR_STORAGE", "/var/lib/pnpr");
    env.set("PNPR_CACHE", "/var/cache/pnpr");
    env.set("PNPR_PUBLIC_URL", "https://registry.example.com");
    env.set("PNPR_PACKUMENT_TTL_SECS", "90");
    env.set("PNPR_OSV", "1");
    env.set("PNPR_OSV_DB", "/var/cache/osv/all.zip");
    env.set("PNPR_DISABLE_REGISTRY", "yes");
    env.set("PNPR_DISABLE_RESOLVER", "true");
    env.set("PNPR_DISABLE_ARTIFACTS", "on");

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert_eq!(args.config.as_deref(), Some("/etc/pnpr/config.yaml".as_ref()));
    assert_eq!(args.listen, "0.0.0.0:4873".parse::<SocketAddr>().unwrap());
    assert_eq!(args.storage.as_deref(), Some("/var/lib/pnpr".as_ref()));
    assert_eq!(args.cache.as_deref(), Some("/var/cache/pnpr".as_ref()));
    assert_eq!(args.public_url.as_deref(), Some("https://registry.example.com"));
    assert_eq!(args.packument_ttl_secs, Some(90));
    assert!(args.osv);
    assert_eq!(args.osv_db.as_deref(), Some("/var/cache/osv/all.zip".as_ref()));
    assert!(args.disable_registry);
    assert!(args.disable_resolver);
    assert!(args.disable_artifacts);
}

#[test]
fn omitted_flags_without_env_vars_keep_their_defaults() {
    let _env = scrubbed_env();

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert_eq!(args.listen, super::Config::DEFAULT_LISTEN.parse::<SocketAddr>().unwrap());
    assert_eq!(args.config, None);
    assert_eq!(args.packument_ttl_secs, None);
    assert!(!args.osv);
    assert!(!args.disable_registry);
    assert!(!args.disable_resolver);
    assert!(!args.disable_artifacts);
}

#[test]
fn flags_on_the_command_line_win_over_env_vars() {
    let env = scrubbed_env();
    env.set("PNPR_LISTEN", "0.0.0.0:4873");
    env.set("PNPR_PACKUMENT_TTL_SECS", "90");
    env.set("PNPR_DISABLE_ARTIFACTS", "false");

    let args = Args::try_parse_from([
        "pnpr",
        "--listen",
        "127.0.0.1:7677",
        "--packument-ttl-secs",
        "5",
        "--disable-artifacts",
    ])
    .unwrap();

    assert_eq!(args.listen, "127.0.0.1:7677".parse::<SocketAddr>().unwrap());
    assert_eq!(args.packument_ttl_secs, Some(5));
    assert!(args.disable_artifacts);
}

#[test]
fn falsy_boolean_env_values_leave_the_flag_off() {
    let env = scrubbed_env();
    env.set("PNPR_OSV", "false");
    env.set("PNPR_DISABLE_REGISTRY", "0");
    env.set("PNPR_DISABLE_RESOLVER", "no");
    env.set("PNPR_DISABLE_ARTIFACTS", "off");

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert!(!args.osv);
    assert!(!args.disable_registry);
    assert!(!args.disable_resolver);
    assert!(!args.disable_artifacts);
}

#[test]
fn invalid_env_values_are_rejected() {
    let env = scrubbed_env();

    env.set("PNPR_OSV", "maybe");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'maybe' for '--osv'"), "{err}");
    env.set("PNPR_OSV", "true");

    env.set("PNPR_LISTEN", "not-an-address");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'not-an-address' for '--listen"), "{err}");
    env.set("PNPR_LISTEN", "127.0.0.1:7677");

    env.set("PNPR_PACKUMENT_TTL_SECS", "soon");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'soon' for '--packument-ttl-secs"), "{err}");
}

#[test]
fn startup_error_report_redacts_dsn_credentials() {
    let err = RegistryError::Internal {
        reason: "startup failed for postgres://admin:secret@[::1]/pnpr?sslmode=require".to_string(),
    };
    let report = redacted_report(&err).to_string();

    assert!(report.contains("postgres://redacted@[::1]/pnpr?sslmode=require"));
    assert!(!report.contains("admin"));
    assert!(!report.contains("secret"));
}
