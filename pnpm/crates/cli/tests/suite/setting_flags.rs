//! Settings spelled as bare `--<setting>` command-line flags.
//!
//! pnpm types every setting for its argument parser, so each one is
//! accepted on the command line as well as in `pnpm-workspace.yaml`
//! ([pnpm/pnpm#14281](https://github.com/pnpm/pnpm/issues/14281)).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{fs, process::Command};

/// Every setting flag, the value it is given, and what `pnpm config get`
/// reports back for it.
const SETTING_FLAGS: [(&[&str], &str, &str); 17] = [
    (&["--package-import-method", "hardlink"], "package-import-method", "hardlink"),
    (&["--hoist-pattern=eslint"], "hoist-pattern", "[\n  \"eslint\"\n]"),
    (&["--public-hoist-pattern=@types/*"], "public-hoist-pattern", "[\n  \"@types/*\"\n]"),
    (&["--no-hoist"], "hoist", "false"),
    (&["--global-dir=/custom/global"], "global-dir", "/custom/global"),
    (&["--virtual-store-dir=custom_store"], "virtual-store-dir", "custom_store"),
    (&["--modules-dir=custom_modules"], "modules-dir", "custom_modules"),
    (&["--child-concurrency=3"], "child-concurrency", "3"),
    (&["--no-lockfile"], "lockfile", "false"),
    (&["--strict-peer-dependencies"], "strict-peer-dependencies", "true"),
    (&["--side-effects-cache"], "side-effects-cache", "true"),
    (&["--side-effects-cache-readonly"], "side-effects-cache-readonly", "true"),
    (&["--trust-policy", "no-downgrade"], "trust-policy", "no-downgrade"),
    (&["--trust-policy-exclude=lodash"], "trust-policy-exclude", "[\n  \"lodash\"\n]"),
    (&["--trust-policy-ignore-after=5"], "trust-policy-ignore-after", "5"),
    (&["--optimistic-repeat-install"], "optimistic-repeat-install", "true"),
    (&["--shamefully-hoist"], "shamefully-hoist", "true"),
];

#[test]
fn every_setting_flag_reaches_the_config() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");

    for (flag, setting, expected) in SETTING_FLAGS {
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_args(flag)
            .with_args(["config", "get", setting])
            .output()
            .expect("run pacquet config get");
        eprintln!("{flag:?} stderr={}", String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "{flag:?} was rejected");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), expected, "{flag:?}");
    }

    drop(root);
}
