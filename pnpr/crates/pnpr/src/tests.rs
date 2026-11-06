use super::{Args, RegistryError, StorageArgs, apply_storage_overrides, redacted_report};
use clap::{CommandFactory as _, Parser as _};
use pnpm_testing_utils::env_guard::EnvGuard;
use std::{ffi::OsStr, net::SocketAddr, time::Duration};

const ENV_VARS: [&str; 12] = [
    "PNPR_CONFIG",
    "PNPR_STORAGE",
    "PNPR_CACHE",
    "PNPR_LISTEN",
    "PNPR_PUBLIC_URL",
    "PNPR_PACKUMENT_TTL",
    "PNPR_OSV",
    "PNPR_OSV_DB",
    "PNPR_DISABLE_REGISTRY",
    "PNPR_DISABLE_RESOLVER",
    "PNPR_DISABLE_ARTIFACTS",
    "PNPR_DISABLE_PIPELINE",
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

    let overrides = args.server.features.feature_overrides();

    assert!(overrides.disable_artifacts);
    assert!(!overrides.disable_registry);
    assert!(!overrides.disable_resolver);
    assert!(!overrides.disable_pipeline);
}

#[test]
fn env_vars_stand_in_for_omitted_flags() {
    let env = scrubbed_env();
    env.set("PNPR_CONFIG", "/etc/pnpr/config.yaml");
    env.set("PNPR_LISTEN", "0.0.0.0:4873");
    env.set("PNPR_STORAGE", "/var/lib/pnpr");
    env.set("PNPR_CACHE", "/var/cache/pnpr");
    env.set("PNPR_PUBLIC_URL", "https://registry.example.com");
    env.set("PNPR_PACKUMENT_TTL", "90s");
    env.set("PNPR_OSV", "1");
    env.set("PNPR_OSV_DB", "/var/cache/osv/all.zip");
    env.set("PNPR_DISABLE_REGISTRY", "yes");
    env.set("PNPR_DISABLE_RESOLVER", "true");
    env.set("PNPR_DISABLE_ARTIFACTS", "on");
    env.set("PNPR_DISABLE_PIPELINE", "1");

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert_eq!(args.config.as_deref(), Some("/etc/pnpr/config.yaml".as_ref()));
    assert_eq!(args.server.listen, "0.0.0.0:4873".parse::<SocketAddr>().unwrap());
    assert_eq!(args.paths.storage.as_deref(), Some("/var/lib/pnpr".as_ref()));
    assert_eq!(args.paths.cache.as_deref(), Some("/var/cache/pnpr".as_ref()));
    assert_eq!(args.server.public_url.as_deref(), Some("https://registry.example.com"));
    assert_eq!(args.server.packument_ttl, Some(Duration::from_secs(90)));
    assert_eq!(args.server.osv_options.osv, Some(true));
    assert_eq!(args.server.osv_options.osv_db.as_deref(), Some("/var/cache/osv/all.zip".as_ref()));
    assert!(args.server.features.disable_registry);
    assert!(args.server.features.disable_resolver);
    assert!(args.server.features.disable_artifacts);
    assert!(args.server.features.disable_pipeline);
}

#[test]
fn omitted_flags_without_env_vars_keep_their_defaults() {
    let _env = scrubbed_env();

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert_eq!(args.server.listen, super::Config::DEFAULT_LISTEN.parse::<SocketAddr>().unwrap());
    assert_eq!(args.config, None);
    assert_eq!(args.server.packument_ttl, None);
    assert_eq!(args.server.osv_options.osv, None);
    assert!(!args.server.features.disable_registry);
    assert!(!args.server.features.disable_resolver);
    assert!(!args.server.features.disable_artifacts);
    assert!(!args.server.features.disable_pipeline);
}

#[test]
fn flags_on_the_command_line_win_over_env_vars() {
    let env = scrubbed_env();
    env.set("PNPR_LISTEN", "0.0.0.0:4873");
    env.set("PNPR_PACKUMENT_TTL", "90s");
    env.set("PNPR_DISABLE_ARTIFACTS", "false");

    let args = Args::try_parse_from([
        "pnpr",
        "--listen",
        "127.0.0.1:7677",
        "--packument-ttl",
        "5s",
        "--disable-artifacts",
    ])
    .unwrap();

    assert_eq!(args.server.listen, "127.0.0.1:7677".parse::<SocketAddr>().unwrap());
    assert_eq!(args.server.packument_ttl, Some(Duration::from_secs(5)));
    assert!(args.server.features.disable_artifacts);
}

#[test]
fn falsy_boolean_env_values_leave_the_flag_off() {
    let env = scrubbed_env();
    env.set("PNPR_OSV", "false");
    env.set("PNPR_DISABLE_REGISTRY", "0");
    env.set("PNPR_DISABLE_RESOLVER", "no");
    env.set("PNPR_DISABLE_ARTIFACTS", "off");
    env.set("PNPR_DISABLE_PIPELINE", "false");

    let args = Args::try_parse_from(["pnpr"]).unwrap();

    assert_eq!(args.server.osv_options.osv, Some(false));
    assert!(!args.server.features.disable_registry);
    assert!(!args.server.features.disable_resolver);
    assert!(!args.server.features.disable_artifacts);
    assert!(!args.server.features.disable_pipeline);
}

#[test]
fn invalid_env_values_are_rejected() {
    let env = scrubbed_env();

    env.set("PNPR_OSV", "maybe");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'maybe' for '--osv"), "{err}");
    env.set("PNPR_OSV", "true");

    env.set("PNPR_LISTEN", "not-an-address");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'not-an-address' for '--listen"), "{err}");
    env.set("PNPR_LISTEN", "127.0.0.1:7677");

    env.set("PNPR_PACKUMENT_TTL", "soon");
    let err = Args::try_parse_from(["pnpr"]).unwrap_err().to_string();
    assert!(err.contains("'soon' for '--packument-ttl"), "{err}");
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

#[test]
fn gc_parses_registries_and_maintenance_options() {
    let _env = scrubbed_env();
    assert!(Args::try_parse_from(["pnpr", "oci-gc"]).is_err());
    let args = Args::try_parse_from(["pnpr", "gc"]).unwrap();
    assert!(
        matches!(args.command, Some(super::Command::Gc { registry, dry_run: false, min_age }) if registry.is_empty() && min_age == Duration::from_hours(24)),
    );
    let args = Args::try_parse_from([
        "pnpr",
        "-c",
        "registry.yaml",
        "gc",
        "--registry",
        "images",
        "--registry",
        "local",
        "--dry-run",
        "--min-age",
        "0",
    ])
    .unwrap();
    assert!(
        matches!(args.command, Some(super::Command::Gc { registry, dry_run: true, min_age }) if registry == ["images", "local"] && min_age.is_zero()),
    );
    assert!(Args::try_parse_from(["pnpr", "gc", "@scope/pkg@1.0.0"]).is_err());
    assert!(Args::try_parse_from(["pnpr", "verify", "--prune-orphans"]).is_err());
}

#[test]
fn cache_gc_parses_maintenance_options() {
    let _env = scrubbed_env();
    let args = Args::try_parse_from([
        "pnpr",
        "cache-gc",
        "--dry-run",
        "--max-size",
        "10GB",
        "--max-age",
        "3600",
        "--orphans-only",
    ])
    .unwrap();
    assert!(matches!(
        args.command,
        Some(super::Command::CacheGc {
            dry_run: true,
            max_size: Some(ref size),
            max_age: Some(d),
            orphans_only: true,
        }) if size == "10GB" && d.as_secs() == 3600
    ));
}

#[test]
fn pnpr_serve_subcommand_works_as_alias() {
    let _env = scrubbed_env();
    let args = Args::try_parse_from(["pnpr", "serve", "--listen", "0.0.0.0:4873"]).unwrap();
    assert!(
        matches!(args.command, Some(super::Command::Serve(server)) if server.listen.to_string() == "0.0.0.0:4873"),
    );
}

#[test]
fn maintenance_subcommands_do_not_accept_server_flags() {
    let _env = scrubbed_env();
    let err =
        Args::try_parse_from(["pnpr", "gc", "--registry", "images", "--listen", "0.0.0.0:4873"])
            .unwrap_err()
            .to_string();
    assert!(err.contains("unexpected argument '--listen'"), "{err}");
}

#[test]
fn interval_valued_flags_accept_units_and_bare_seconds() {
    let _env = scrubbed_env();
    let args = Args::try_parse_from(["pnpr", "--packument-ttl", "5m"]).unwrap();
    assert_eq!(args.server.packument_ttl, Some(Duration::from_mins(5)));

    let args2 = Args::try_parse_from(["pnpr", "--packument-ttl", "300"]).unwrap();
    assert_eq!(args2.server.packument_ttl, Some(Duration::from_mins(5)));

    let gc_args =
        Args::try_parse_from(["pnpr", "gc", "--registry", "img", "--min-age", "2h"]).unwrap();
    assert!(
        matches!(gc_args.command, Some(super::Command::Gc { min_age, .. }) if min_age == Duration::from_hours(2)),
    );
}

/// Load `yaml` as a config file and serve it from `storage` instead.
fn served_from(yaml: &str, storage: &std::path::Path) -> pnpr::Config {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let mut config =
        pnpr::Config::from_yaml(&path, "127.0.0.1:4873".parse().unwrap(), None).unwrap();
    let mut paths = StorageArgs { storage: Some(storage.to_path_buf()), cache: None };
    apply_storage_overrides(&mut config, &mut paths, &pnpr::ConfigSource::Cli(path));
    config
}

#[test]
fn a_storage_override_takes_the_default_metadata_database_along() {
    let storage = std::path::Path::new("/srv/pnpr");
    let config = served_from("storage: ./storage\n", storage);
    assert_eq!(
        config.storage.metadata,
        pnpr_config::MetadataBackend::Sqlite(pnpr_config::default_metadata_path(storage)),
    );
}

#[test]
fn a_storage_override_leaves_a_named_metadata_database_where_it_is() {
    let named = tempfile::TempDir::new().unwrap();
    let database = named.path().join("metadata.sqlite");
    let yaml = format!(
        "storage: ./storage\nmetadata:\n  url: 'sqlite:{}'\n",
        database
            .display()
            .to_string()
            .replace('\'', "''"),
    );
    let config = served_from(&yaml, std::path::Path::new("/srv/pnpr"));
    assert_eq!(config.storage.metadata, pnpr_config::MetadataBackend::Sqlite(database));
}

/// A database the config names at the default path is the storage
/// directory's own, so it moves the same way.
#[test]
fn a_storage_override_takes_a_database_named_at_the_default_path_along() {
    let storage = std::path::Path::new("/srv/pnpr");
    let config = served_from(
        "storage: ./storage\nmetadata:\n  url: sqlite:./storage/.metadata/metadata.sqlite\n",
        storage,
    );
    assert_eq!(
        config.storage.metadata,
        pnpr_config::MetadataBackend::Sqlite(pnpr_config::default_metadata_path(storage)),
    );
}
