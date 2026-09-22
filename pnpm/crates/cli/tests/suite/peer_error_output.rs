use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::fs;

const ERROR_BLOCK: &str = r#"[ERR_PNPM_PEER_DEP_ISSUES] Unmet peer dependencies

✕ missing peer missing-peer
  Wanted:
    "*":
      peer-user@file:dep
hint: To auto-install peer dependencies, add the following to "pnpm-workspace.yaml" in your project root:

  autoInstallPeers: true
hint: To disable failing on peer dependency issues, add the following to pnpm-workspace.yaml in your project root:

  strictPeerDependencies: false
"#;

fn peer_project(strict: bool) -> CommandTempCwd<()> {
    let project = CommandTempCwd::init();
    fs::create_dir(project.workspace.join("dep")).expect("create dependency directory");
    fs::write(
        project.workspace.join("package.json"),
        r#"{"name":"repro","version":"1.0.0","dependencies":{"peer-user":"file:./dep"}}"#,
    )
    .expect("write project manifest");
    fs::write(
        project.workspace.join("dep/package.json"),
        r#"{"name":"peer-user","version":"1.0.0","peerDependencies":{"missing-peer":"*"}}"#,
    )
    .expect("write dependency manifest");
    fs::write(
        project.workspace.join("pnpm-workspace.yaml"),
        format!("autoInstallPeers: false\nstrictPeerDependencies: {strict}\n"),
    )
    .expect("write workspace settings");
    project
}

#[test]
fn fatal_peer_block_and_hints_go_only_to_stderr() {
    for args in [vec![], vec!["--reporter=append-only"], vec!["--use-stderr"]] {
        let mut project = peer_project(true);
        let output = project.pacquet
            .args(["install", "--color=never"])
            .args(args)
            .output()
            .expect("run install");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(stderr.ends_with(&format!("{ERROR_BLOCK}\n")), "stderr:\n{stderr}");
        for part in ["ERR_PNPM_PEER_DEP_ISSUES", "missing peer", "hint:", "strictPeerDependencies"]
        {
            assert!(!stdout.contains(part), "stdout:\n{stdout}");
        }
        assert_eq!(stderr.matches("ERR_PNPM_PEER_DEP_ISSUES").count(), 1);
    }
}

#[test]
fn nonfatal_peer_warning_stays_on_stdout() {
    let mut project = peer_project(false);
    let output = project.pacquet
        .arg("install")
        .output()
        .expect("run install");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("[WARN] Issues with peer dependencies found."), "{stdout}");
    assert!(!stdout.contains("ERR_PNPM_PEER_DEP_ISSUES"), "{stdout}");
}

#[test]
fn fatal_peer_ndjson_keeps_the_structured_error() {
    let project = peer_project(true);
    let output = project.pacquet
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("run install");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    let records: Vec<Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).expect("every stderr line is JSON"))
        .collect();
    let errors: Vec<_> = records
        .iter()
        .filter(|record| record["level"] == "error")
        .collect();
    assert_eq!(errors.len(), 1, "{records:?}");
    assert_eq!(errors[0]["name"], "pnpm:global");
    assert_eq!(errors[0]["message"], ERROR_BLOCK);
}

#[test]
fn silent_peer_failure_stays_silent() {
    let project = peer_project(true);
    let output = project.pacquet
        .with_args(["install", "--reporter=silent"])
        .output()
        .expect("run install");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn registry_not_found_stays_on_stderr() {
    let mut project = CommandTempCwd::init().add_mocked_registry();
    fs::write(
        project.workspace.join("package.json"),
        r#"{"dependencies":{"pnpm-missing-peer-error-test":"1.0.0"}}"#,
    )
    .expect("write project manifest");
    let output = project.pacquet
        .arg("install")
        .output()
        .expect("run install");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("ERR_PNPM_FETCH_404"), "{stderr}");
    assert!(!stdout.contains("ERR_PNPM_FETCH_404"), "{stdout}");
}
