//! `pnpm env` — the deprecated Node.js front end to `pnpm runtime`.
//!
//! Only the paths that stop before the network are covered here; `env use`
//! and `env list` reach the Node.js mirror, which the mocked registry does
//! not serve.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

struct EnvOutput {
    succeeded: bool,
    /// `env` is not one of pnpm's stderr-reporter commands, so its reporter
    /// output lands here while the miette error goes to stderr.
    stdout: String,
    stderr: String,
}

fn run_env(args: &[&str]) -> EnvOutput {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet
        .with_args(args)
        .output()
        .unwrap_or_else(|error| panic!("run pacquet env {args:?}: {error}"));
    let outcome = EnvOutput {
        succeeded: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    eprintln!("{args:?} stdout={} stderr={}", outcome.stdout, outcome.stderr);
    drop(root);
    outcome
}

#[test]
fn a_bare_env_asks_for_a_subcommand() {
    let output = run_env(&["env"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_ENV_NO_SUBCOMMAND"), "{}", output.stderr);
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    let output = run_env(&["env", "install"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND"), "{}", output.stderr);
}

/// pnpm warns before it validates, so the deprecation notice reaches a user
/// whose invocation is about to be rejected for another reason.
#[test]
fn use_without_global_is_refused_after_the_deprecation_warning() {
    let output = run_env(&["env", "use", "24"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_NOT_IMPLEMENTED_YET"), "{}", output.stderr);
    assert!(output.stdout.contains(r#""pnpm env use" is deprecated"#), "{}", output.stdout);
}
