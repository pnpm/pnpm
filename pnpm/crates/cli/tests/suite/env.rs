//! `pnpm env` — the deprecated Node.js front end to `pnpm runtime`.
//!
//! Only the paths that stop before the network are covered here; `env use`
//! and `env list` reach the Node.js mirror, which the mocked registry does
//! not serve.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

fn env_stderr(args: &[&str]) -> (bool, String) {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet
        .with_args(args)
        .output()
        .unwrap_or_else(|error| panic!("run pacquet env {args:?}: {error}"));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("{args:?} stderr={stderr}");
    let succeeded = output.status.success();
    drop(root);
    (succeeded, stderr)
}

#[test]
fn a_bare_env_asks_for_a_subcommand() {
    let (succeeded, stderr) = env_stderr(&["env"]);
    assert!(!succeeded);
    assert!(stderr.contains("ERR_PNPM_ENV_NO_SUBCOMMAND"), "{stderr}");
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    let (succeeded, stderr) = env_stderr(&["env", "install"]);
    assert!(!succeeded);
    assert!(stderr.contains("ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND"), "{stderr}");
}

#[test]
fn use_without_global_is_refused() {
    let (succeeded, stderr) = env_stderr(&["env", "use", "24"]);
    assert!(!succeeded);
    assert!(stderr.contains("ERR_PNPM_NOT_IMPLEMENTED_YET"), "{stderr}");
}
