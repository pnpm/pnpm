use super::{SwitchInput, SwitchProcessState, SwitchSource, switch_plan_from_input, switch_target};
use crate::config_overrides::ConfigOverrides;
use pacquet_config::{Config, PmOnFail};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

#[test]
fn version_argv_reads_dir_auth_file_and_command_forms() {
    struct Case {
        name: &'static str,
        argv: &'static [&'static str],
        dir: &'static str,
        npmrc_auth_file: Option<&'static str>,
        command: Option<&'static str>,
    }

    let cases = [
        Case {
            name: "separate long dir and equals auth file",
            argv: &["pnpm", "--dir", "/tmp/project", "--npmrc-auth-file=auth.ini", "--version"],
            dir: "/tmp/project",
            npmrc_auth_file: Some("auth.ini"),
            command: None,
        },
        Case {
            name: "short dir",
            argv: &["pnpm", "-C", "/tmp/short-dir", "--version"],
            dir: "/tmp/short-dir",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "equals dir and userconfig alias",
            argv: &["pnpm", "--dir=/tmp/equals-dir", "--userconfig", "user.ini", "--version"],
            dir: "/tmp/equals-dir",
            npmrc_auth_file: Some("user.ini"),
            command: None,
        },
        Case {
            name: "separator stops command detection",
            argv: &["pnpm", "--dir=/tmp/separator", "--", "run"],
            dir: "/tmp/separator",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "value-taking global option is skipped",
            argv: &["pnpm", "--filter", "pkg", "--reporter", "append-only", "install"],
            dir: ".",
            npmrc_auth_file: None,
            command: Some("install"),
        },
    ];

    for case in cases {
        let argv = case.argv.iter().copied().map(OsString::from).collect::<Vec<_>>();
        let input = SwitchInput::from_version_argv(&argv);

        assert_eq!(input.dir, PathBuf::from(case.dir), "case: {}", case.name);
        assert_eq!(
            input.npmrc_auth_file,
            case.npmrc_auth_file.map(PathBuf::from),
            "case: {}",
            case.name,
        );
        assert_eq!(input.command.as_deref(), case.command, "case: {}", case.name);
    }
}

#[test]
fn switch_plan_skips_when_executed_by_corepack() {
    let input =
        SwitchInput { dir: PathBuf::from("missing-project"), npmrc_auth_file: None, command: None };

    let plan = switch_plan_from_input(
        &input,
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: true },
    )
    .expect("switch plan");

    assert!(plan.is_none(), "unexpected switch plan when executed by Corepack");
}

#[test]
fn switch_target_prefers_locked_dev_engine_version() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"^11.0.0-rc.5","onFail":"download"}}}"#,
    );
    write_lockfile(
        root.path(),
        r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 11.1.2
        version: 11.1.2
      pnpm:
        specifier: 11.1.2
        version: 11.1.2

packages:

  '@pnpm/exe@11.1.2':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@11.1.2:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@11.1.2': {}

  pnpm@11.1.2: {}
---
",
    );

    let target = switch_target(&Config::default(), root.path()).expect("target").expect("switch");

    assert_eq!(target.spec, "^11.0.0-rc.5");
    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "11.1.2");
}

#[test]
fn switch_target_accepts_peer_suffixed_package_manager_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_PEER_SUFFIX);

    let target = switch_target(&Config::default(), root.path()).expect("target").expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "9.3.0");
}

#[test]
fn switch_target_accepts_v12_lockfile_without_legacy_wrapper_entry() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@99.0.0"}"#);
    write_lockfile(root.path(), LOCKED_99_0_0);

    let target = switch_target(&Config::default(), root.path()).expect("target").expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "99.0.0");
}

#[test]
fn switch_target_rejects_package_manager_lockfile_resolution_with_non_integrity_fields() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_TARBALL_RESOLUTION);

    let error = switch_target_error(root.path());

    assert!(error.to_string().contains("integrity-only resolution"), "unexpected error: {error:?}");
}

#[test]
fn switch_target_rejects_package_manager_lockfile_dependency_with_non_registry_dep_path() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_FILE_DEP_PATH);

    let error = switch_target_error(root.path());

    assert!(error.to_string().contains("registry package path"), "unexpected error: {error:?}");
}

#[test]
fn switch_target_reresolves_when_locked_version_no_longer_satisfies_range() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=9.1.2 <9.1.4");
    write_lockfile(root.path(), LOCKED_9_1_1);

    let target = switch_target(&Config::default(), root.path()).expect("target").expect("switch");

    assert_eq!(target.spec, ">=9.1.2 <9.1.4");
    let SwitchSource::Resolve { env_root } = target.source else {
        panic!("expected resolve target");
    };
    assert_eq!(env_root, root.path());
}

#[test]
fn switch_target_uses_global_env_for_legacy_package_manager_field() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config { global_pkg_dir: Some(global_pkg_dir.clone()), ..Config::default() },
        root.path(),
    )
    .expect("target")
    .expect("switch");

    let SwitchSource::Resolve { env_root } = target.source else {
        panic!("expected resolve target");
    };
    assert_eq!(target.spec, "9.3.0");
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_respects_pm_on_fail_ignore() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config {
            pm_on_fail: Some(PmOnFail::Ignore),
            global_pkg_dir: Some(root.path().join("pnpm-home").join("global")),
            ..Config::default()
        },
        root.path(),
    )
    .expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}

#[test]
fn switch_target_does_not_switch_dev_engine_without_download() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"9.3.0","onFail":"error"}}}"#,
    );

    let target = switch_target(&Config::default(), root.path()).expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}

fn write_dev_engine_manifest(root: &Path, version: &str) {
    write_manifest(
        root,
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{version}","onFail":"download"}}}}}}"#,
        ),
    );
}

fn write_manifest(root: &Path, content: &str) {
    fs::write(root.join("package.json"), content).expect("write manifest");
}

fn write_lockfile(root: &Path, content: &str) {
    fs::write(root.join("pnpm-lock.yaml"), content).expect("write lockfile");
}

fn switch_target_error(root: &Path) -> miette::Report {
    match switch_target(&Config::default(), root) {
        Ok(_) => panic!("expected poisoned lockfile to fail"),
        Err(error) => error,
    }
}

const LOCKED_9_3_0_WITH_PEER_SUFFIX: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  '@pnpm/linux-x64@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  peer-provider@1.0.0:
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0':
    optionalDependencies:
      '@pnpm/linux-x64': 9.3.0(peer-provider@1.0.0)

  '@pnpm/linux-x64@9.3.0(peer-provider@1.0.0)':
    dependencies:
      peer-provider: 1.0.0
    optional: true

  peer-provider@1.0.0: {}

  pnpm@9.3.0: {}
---
";

const LOCKED_9_3_0_WITH_TARBALL_RESOLUTION: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  '@pnpm/linux-x64@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==, tarball: https://evil.example.com/pnpm-linux-x64.tgz}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0':
    optionalDependencies:
      '@pnpm/linux-x64': 9.3.0

  '@pnpm/linux-x64@9.3.0':
    optional: true

  pnpm@9.3.0: {}
---
";

const LOCKED_9_3_0_WITH_FILE_DEP_PATH: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  payload@file:../payload.tgz:
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0': {}

  payload@file:../payload.tgz: {}

  pnpm@9.3.0:
    dependencies:
      payload: file:../payload.tgz
---
";

const LOCKED_9_1_1: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: '>=9.1.0 <9.1.2'
        version: 9.1.1
      pnpm:
        specifier: '>=9.1.0 <9.1.2'
        version: 9.1.1

packages:

  '@pnpm/exe@9.1.1':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.1.1:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.1.1': {}

  pnpm@9.1.1: {}
---
";

const LOCKED_99_0_0: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: 99.0.0
        version: 99.0.0

packages:

  pnpm@99.0.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  pnpm@99.0.0: {}
---
";
