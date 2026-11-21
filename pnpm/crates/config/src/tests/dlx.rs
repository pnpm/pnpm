use super::{
    Config,
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    LinkProbe,
    OsString,
    Path,
    PathBuf,
    StoreDir,
    TrustPolicy,
    assert_eq,
    fs,
    io,
    tempdir,
    write_file,
};
use crate::{
    CliLayer,
    CommandTarget,
};

fn dlx_load() -> Config {
    Config {
        cli: CliLayer { command_target: CommandTarget::Dlx, ..CliLayer::default() },
        ..Config::default()
    }
}

#[test]
fn dlx_load_isolates_untrusted_workspace_execution_settings_and_preserves_global_config() {
    fake_env!();
    let home = tempdir().expect("home tempdir");
    let xdg = tempdir().expect("xdg tempdir");
    let workspace = tempdir().expect("workspace tempdir");

    let global_cache = home.path().join("user-cache");
    let global_store = home.path().join("user-store");
    let global_hook = home.path().join("user-hook.cjs");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(
        &config_dir.join("config.yaml"),
        &format!(
            "cacheDir: {}\n\
storeDir: {}\n\
dlxCacheMaxAge: 42\n\
blockExoticSubdeps: true\n\
globalPnpmfile: {}\n\
scriptShell: /bin/user-sh\n\
nodeDownloadMirrors:\n  release: https://global.example/release/\n  nightly: https://global.example/nightly/\n",
            global_cache.display(),
            global_store.display(),
            global_hook.display(),
        ),
    );

    write_file(
        &workspace.path().join("pnpm-workspace.yaml"),
        r"pnpmfile: ./evil-hook.cjs
globalPnpmfile: ./evil-global-hook.cjs
configDependencies:
  evil-plugin: 1.0.0+sha512-deadbeef
scriptShell: ./evil-shell.sh
shellEmulator: true
cacheDir: ./evil-cache
storeDir: ./evil-store
dlxCacheMaxAge: 99999
ignoreScripts: true
blockExoticSubdeps: false
overrides:
  foo: 2.0.0
packageExtensions:
  bar:
    dependencies:
      baz: 1.0.0
allowBuilds:
  evil-pkg: true
dangerouslyAllowAllBuilds: true
registry: https://workspace.example/npm/
minimumReleaseAge: 120
minimumReleaseAgeExclude:
  - trusted-pkg
minimumReleaseAgeStrict: true
referenceTime: '2026-01-01T00:00:00Z'
trustPolicy: no-downgrade
trustPolicyExclude:
  - legacy-pkg
trustPolicyIgnoreAfter: 60
trustLockfile: true
fetchRetries: 5
fetchTimeout: 12345
nodeDownloadMirrors:
  release: https://workspace.example/release/
  nightly: https://workspace.example/nightly/
  rc: https://workspace.example/rc/
",
    );
    let evil_cert = workspace.path().join("evil-cert.pem");
    write_file(&evil_cert, "-----BEGIN CERTIFICATE-----\nevil\n-----END CERTIFICATE-----\n");
    write_file(
        &workspace.path().join(".npmrc"),
        &format!(
            "//workspace.example/npm/:_authToken=ws-secret\n\
https-proxy=http://evil-proxy.example:8080\n\
strict-ssl=false\n\
local-address=127.0.0.2\n\
ca=-----BEGIN CERTIFICATE-----\\nevil-ca\\n-----END CERTIFICATE-----\n\
cafile={}\n\
cert=-----BEGIN CERTIFICATE-----\\nevil-client\\n-----END CERTIFICATE-----\n\
key=-----BEGIN PRIVATE KEY-----\\nevil-key\\n-----END PRIVATE KEY-----\n\
//workspace.example/npm/:certfile={}\n",
            evil_cert.display(),
            evil_cert.display(),
        ),
    );

    set_fake_env(&[
        (
            "HOME",
            home.path()
                .to_str()
                .expect("UTF-8 path"),
        ),
        ("XDG_CONFIG_HOME", xdg.path().to_str().expect("UTF-8 path")),
    ]);

    let config = dlx_load().current::<FakeEnv>(workspace.path()).expect("load dlx config");

    assert_eq!(config.pnpmfile, None);
    assert_eq!(config.global_pnpmfile, Some(global_hook));
    assert_eq!(config.config_dependencies, None);
    assert_eq!(config.script_shell.as_deref(), Some("/bin/user-sh"));
    assert!(!config.shell_emulator);
    assert_eq!(config.cache_dir, global_cache);
    assert_eq!(config.store_dir, StoreDir::from(global_store));
    assert_eq!(config.dlx_cache_max_age, 42);
    assert!(!config.ignore_scripts);
    assert!(config.block_exotic_subdeps);
    assert_eq!(config.overrides, None);
    assert_eq!(config.package_extensions, None);
    assert!(config.allow_builds.is_empty());
    assert!(!config.dangerously_allow_all_builds);
    assert!(config.extra_bin_paths.is_empty());
    assert_eq!(config.proxy, pnpm_network::ProxyConfig::default());
    assert_eq!(config.tls.strict_ssl, None);
    assert_eq!(config.tls.local_address, None);
    assert!(config.tls.ca.is_empty());
    assert!(config.tls.cert.is_none());
    assert!(config.tls.key.is_none());
    assert!(config.tls_by_uri.is_empty());
    assert!(!config.raw_auth_config.contains_key("cafile"));
    assert!(!config.raw_auth_config.contains_key("//workspace.example/npm/:certfile"));

    assert_eq!(config.registry, "https://workspace.example/npm/");
    assert_eq!(config.minimum_release_age, Some(120));
    assert_eq!(config.minimum_release_age_exclude, Some(vec!["trusted-pkg".to_string()]));
    assert_eq!(config.minimum_release_age_strict, Some(true));
    assert_eq!(
        config.reference_time,
        Some(pnpm_time::parse_rfc3339("2026-01-01T00:00:00Z").unwrap()),
    );
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude, Some(vec!["legacy-pkg".to_string()]));
    assert_eq!(config.trust_policy_ignore_after, Some(60));
    assert!(config.trust_lockfile);
    assert_eq!(config.fetch_retries, 5);
    assert_eq!(config.fetch_timeout, 12345);
    assert_eq!(
        config.node_download_mirrors.get("release").map(String::as_str),
        Some("https://workspace.example/release/"),
    );
    assert_eq!(
        config.node_download_mirrors.get("nightly").map(String::as_str),
        Some("https://global.example/nightly/"),
    );
    assert!(!config.node_download_mirrors.contains_key("rc"));
}
