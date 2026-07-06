use super::{SwitchInput, SwitchSource, switch_target};
use pacquet_config::Config;
use std::{ffi::OsString, fs, path::PathBuf};
use tempfile::TempDir;

#[test]
fn version_argv_reads_dir_and_auth_file() {
    let input = SwitchInput::from_version_argv(&[
        OsString::from("pnpm"),
        OsString::from("--dir"),
        OsString::from("/tmp/project"),
        OsString::from("--npmrc-auth-file=auth.ini"),
        OsString::from("--version"),
    ]);

    assert_eq!(input.dir, PathBuf::from("/tmp/project"));
    assert_eq!(input.npmrc_auth_file, Some(PathBuf::from("auth.ini")));
    assert_eq!(input.command, None);
}

#[test]
fn switch_target_prefers_locked_dev_engine_version() {
    let root = TempDir::new().expect("tmp dir");
    fs::write(
        root.path().join("package.json"),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"^11.0.0-rc.5","onFail":"download"}}}"#,
    )
    .expect("write manifest");
    fs::write(
        root.path().join("pnpm-lock.yaml"),
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
    )
    .expect("write lockfile");

    let target = switch_target(&Config::default(), root.path()).expect("target").expect("switch");

    assert_eq!(target.spec, "^11.0.0-rc.5");
    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "11.1.2");
}
