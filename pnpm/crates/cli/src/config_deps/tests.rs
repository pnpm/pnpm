use std::{fs, path::Path};

use pnpm_config::{Config, Host, PNPM_VERSION, TrustPolicy};
use pnpm_reporter::SilentReporter;

use super::{resolve_engine_version, run_update_config_hooks};

#[tokio::test]
async fn update_config_records_prefer_frozen_lockfile_as_explicit() {
    for prefer_value in [true, false] {
        let root = tempfile::tempdir().expect("workspace tempdir");
        fs::write(root.path().join("pnpm-workspace.yaml"), "\n").expect("write workspace settings");
        fs::write(
            root.path().join(".pnpmfile.cjs"),
            format!(
                "module.exports = {{ hooks: {{ updateConfig (config) {{ config.preferFrozenLockfile = {prefer_value}; return config }} }} }}",
            ),
        )
        .expect("write pnpmfile");
        let mut config =
            Config::default().current::<Host>(root.path()).expect("load configuration");

        run_update_config_hooks::<SilentReporter>(&mut config, root.path())
            .await
            .expect("run updateConfig hook");

        assert_eq!(config.prefer_frozen_lockfile, prefer_value);
        assert_eq!(
            config
                .explicit_settings
                .get("preferFrozenLockfile")
                .and_then(serde_json::Value::as_bool),
            Some(prefer_value),
        );
    }
}

#[tokio::test]
async fn update_config_null_clears_virtual_store_dir() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(root.path().join("pnpm-workspace.yaml"), "virtualStoreDir: pinned-virtual\n")
        .expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.virtualStoreDir = null; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(config.virtual_store_dir, root.path().join("node_modules/.pnpm"));
    assert!(!config.explicit_settings.contains_key("virtualStoreDir"));
}

#[tokio::test]
async fn update_config_null_clears_global_virtual_store_dir() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "enableGlobalVirtualStore: true\nvirtualStoreDir: pinned-virtual\nglobalVirtualStoreDir: pinned-global\n",
    )
    .expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.globalVirtualStoreDir = null; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(config.global_virtual_store_dir, root.path().join("pinned-virtual"));
    assert!(!config.explicit_settings.contains_key("globalVirtualStoreDir"));
}

#[tokio::test]
async fn update_config_can_extend_extra_bin_paths() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    // A workspace manifest makes `current` seed `extra_bin_paths` with the
    // workspace root's `node_modules/.bin`; the hook appends to that.
    fs::write(root.path().join("pnpm-workspace.yaml"), "\n").expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.extraBinPaths = [...config.extraBinPaths, '/opt/pnpm-build/bin']; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");
    let seeded = config.extra_bin_paths.clone();
    assert_eq!(seeded, vec![root.path().join("node_modules").join(".bin")]);

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    let mut expected = seeded;
    expected.push(Path::new("/opt/pnpm-build/bin").to_path_buf());
    assert_eq!(config.extra_bin_paths, expected);
}

#[tokio::test]
async fn update_config_can_set_extra_env() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(root.path().join("pnpm-workspace.yaml"), "\n").expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, npm_config_nodedir: '/brazil/node' }; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");
    // `current::<Host>` reads the developer's real global config, which
    // may seed `extra_env` (a global virtual store adds `NODE_PATH`), so
    // assert only on the entry the hook adds.
    assert_eq!(config.extra_env.get("npm_config_nodedir"), None);

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(
        config.extra_env.get("npm_config_nodedir").map(String::as_str),
        Some("/brazil/node"),
    );
}

/// A hook receives and returns config values without a workspace-manifest
/// path context. Relative `scriptShell` output therefore stays relative,
/// matching TypeScript's direct adoption of the hook result rather than the
/// manifest reader's one-time path anchoring.
#[tokio::test]
async fn update_config_script_shell_output_is_not_resolved_again() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(root.path().join("pnpm-workspace.yaml"), "scriptShell: ./manifest-shell.sh\n")
        .expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.scriptShell = './hook-shell.sh'; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");
    let expected_manifest_shell =
        root.path().join("manifest-shell.sh").to_string_lossy().into_owned();
    assert_eq!(config.script_shell.as_deref(), Some(expected_manifest_shell.as_str()),);

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(config.script_shell.as_deref(), Some("./hook-shell.sh"));
}

#[tokio::test]
async fn update_config_hook_reads_resolved_script_shell_value() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(root.path().join("pnpm-workspace.yaml"), "scriptShell: ./manifest-shell.sh\n")
        .expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.scriptShell += '-from-hook'; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");
    let expected = format!("{}-from-hook", root.path().join("manifest-shell.sh").display());

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(config.script_shell.as_deref(), Some(expected.as_str()));
}

#[tokio::test]
async fn update_config_hook_deleting_script_shell_clears_value() {
    let root = tempfile::tempdir().expect("workspace tempdir");
    fs::write(root.path().join("pnpm-workspace.yaml"), "scriptShell: ./manifest-shell.sh\n")
        .expect("write workspace settings");
    fs::write(
        root.path().join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { delete config.scriptShell; return config } } }",
    )
    .expect("write pnpmfile");
    let mut config = Config::default().current::<Host>(root.path()).expect("load configuration");
    assert!(config.script_shell.is_some());

    run_update_config_hooks::<SilentReporter>(&mut config, root.path())
        .await
        .expect("run updateConfig hook");

    assert_eq!(config.script_shell, None);
}

/// `Accept` header the resolver sends for full metadata
/// (`ACCEPT_FULL_DOC`); only the full packument carries the per-version
/// `time` map and trust evidence the no-downgrade check reads.
const ACCEPT_FULL: &str = "application/json; q=1.0, */*";

/// `Accept` header the resolver sends for abbreviated metadata
/// (`ACCEPT_ABBREVIATED_DOC`) — what the probe would use if it did *not*
/// derive full metadata from the trust policy.
const ACCEPT_ABBREVIATED: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

/// Full `pnpm` packument: carries the per-version `time` map. `1.0.0` has
/// stronger trust evidence (`trustedPublisher` + provenance) than the
/// later `1.1.0` (none) — a trust downgrade the no-downgrade check must
/// reject once it can see `time`.
const FULL_DOWNGRADE_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "_npmUser": {
                "name": "alice",
                "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
            },
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz",
                "attestations": {
                    "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
                }
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

/// Full `pnpm` packument with no trust downgrade: neither version carries
/// trust evidence, and `time` is present. Resolving `^1.0.0` picks `1.1.0`
/// and the no-downgrade check passes.
const FULL_CLEAN_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

/// Abbreviated packument: no `time` map. Served only to the abbreviated
/// `Accept` header, so the probe reaches it only if it *fails* to request
/// full metadata — in which case the trust check fails closed with
/// "missing time". A test that instead sees a "trust downgrade" (or a
/// clean resolve) proves the probe requested full metadata.
const ABBREVIATED_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

/// Abbreviated packument that *does* carry `time` — what a registry with
/// `registrySupportsTimeField=true` serves. It still omits the trust
/// evidence (`_npmUser` / `dist.attestations`) that no abbreviated
/// packument carries, so a no-downgrade check run against it would see no
/// evidence and miss the downgrade.
const ABBREVIATED_WITH_TIME_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

fn no_downgrade_config(registry: String, cache_dir: &Path) -> Config {
    let mut config = Config {
        trust_policy: TrustPolicy::NoDowngrade,
        cache_dir: cache_dir.to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = registry;
    config
}

/// Under `trustPolicy=no-downgrade`, the self-update probe must resolve
/// against **full** metadata — the same as a regular install — so the
/// no-downgrade check actually runs. It reaches the full packument (with
/// `time`), sees the downgrade, and rejects it. If the probe fetched
/// abbreviated metadata it would instead fail closed with "missing time";
/// if it skipped the check it would resolve `1.1.0` with no error.
#[tokio::test]
async fn resolve_pnpm_version_fetches_full_metadata_and_rejects_a_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _full = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_FULL)
        .with_status(200)
        .with_body(FULL_DOWNGRADE_BODY)
        .create_async()
        .await;
    let _abbreviated = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let config = no_downgrade_config(format!("{}/", server.url()), cache_dir.path());

    let err = resolve_engine_version(&config, "pnpm", "^1.0.0")
        .await
        .expect_err("a trust downgrade must be rejected");
    let report = format!("{err:?}");
    assert!(
        report.contains("trust downgrade"),
        "expected a trust-downgrade rejection, got: {report}",
    );
    assert!(
        !report.contains("missing time"),
        "the probe must fetch full metadata, not fail closed on abbreviated: {report}",
    );
}

/// A normal (non-downgrade) target still resolves under
/// `trustPolicy=no-downgrade`: the probe fetches full metadata, the check
/// passes, and it picks the highest in range.
#[tokio::test]
async fn resolve_pnpm_version_resolves_a_clean_update_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _full = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_FULL)
        .with_status(200)
        .with_body(FULL_CLEAN_BODY)
        .create_async()
        .await;
    let _abbreviated = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let config = no_downgrade_config(format!("{}/", server.url()), cache_dir.path());

    let resolved = resolve_engine_version(&config, "pnpm", "^1.0.0")
        .await
        .expect("a clean update must resolve")
        .expect("a matching pnpm version resolves");

    assert_eq!(resolved.version, "1.1.0");
    assert!(resolved.policy_violation.is_none());
}

/// `registrySupportsTimeField=true` (abbreviated metadata carries `time`)
/// must NOT let the no-downgrade check settle for abbreviated metadata:
/// abbreviated still omits the trust evidence, so the check would see none
/// and miss the downgrade. The probe must fetch full metadata regardless of
/// `registrySupportsTimeField` and reject the downgrade.
#[tokio::test]
async fn resolve_pnpm_version_forces_full_metadata_for_no_downgrade_despite_registry_time_field() {
    let mut server = mockito::Server::new_async().await;
    let _full = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_FULL)
        .with_status(200)
        .with_body(FULL_DOWNGRADE_BODY)
        .create_async()
        .await;
    // Abbreviated here carries `time` (as a `registrySupportsTimeField`
    // registry would) but no trust evidence. If the probe wrongly settled
    // for it, the downgrade would be missed and resolution would succeed.
    let _abbreviated = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        .with_body(ABBREVIATED_WITH_TIME_BODY)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let mut config = no_downgrade_config(format!("{}/", server.url()), cache_dir.path());
    config.registry_supports_time_field = true;

    let err = resolve_engine_version(&config, "pnpm", "^1.0.0")
        .await
        .expect_err("the downgrade must be rejected even with registrySupportsTimeField");
    let report = format!("{err:?}");
    assert!(
        report.contains("trust downgrade"),
        "expected a trust-downgrade rejection, got: {report}",
    );
}

/// pnpm/pnpm#13883: a dist-tag pointing at the running pnpm, published
/// within the `minimumReleaseAge` cutoff, must still resolve to it. The
/// maturity filter moved such a tag back to the previous mature release,
/// so `pnpm self-update <tag>` downgraded instead of doing nothing.
#[tokio::test]
async fn resolve_pnpm_version_keeps_a_dist_tag_on_the_running_version() {
    let running = node_semver::Version::parse(PNPM_VERSION).expect("parse the running version");
    let older = if running.pre_release.is_empty() {
        format!("{}.0.0", running.major - 1)
    } else {
        format!("{}.{}.{}-0", running.major, running.minor, running.patch)
    };
    let body = format!(
        r#"{{
    "name": "pnpm",
    "dist-tags": {{ "latest": "{PNPM_VERSION}" }},
    "time": {{
        "{older}": "2024-01-10T08:30:00.000Z",
        "{PNPM_VERSION}": "{fresh}"
    }},
    "versions": {{
        "{older}": {{
            "name": "pnpm",
            "version": "{older}",
            "dist": {{
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-{older}.tgz"
            }}
        }},
        "{PNPM_VERSION}": {{
            "name": "pnpm",
            "version": "{PNPM_VERSION}",
            "dist": {{
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-{PNPM_VERSION}.tgz"
            }}
        }}
    }}
}}"#,
        fresh = chrono::Utc::now().to_rfc3339(),
    );
    let mut server = mockito::Server::new_async().await;
    let _packument = server
        .mock("GET", "/pnpm")
        .with_status(200)
        .with_body(&body)
        .expect_at_least(1)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let mut config = Config {
        minimum_release_age: Some(24 * 60),
        cache_dir: cache_dir.path().to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = format!("{}/", server.url());

    let resolved = resolve_engine_version(&config, "pnpm", "latest")
        .await
        .expect("the tag must resolve")
        .expect("a matching pnpm version resolves");

    assert_eq!(resolved.version, PNPM_VERSION);
}
